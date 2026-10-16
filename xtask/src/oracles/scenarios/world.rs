// SPDX-License-Identifier: GPL-3.0-or-later

//! Scenario tests of the mock builtins: `differential.md` DIFF-6 and DIFF-7.

use super::cases::{done, f, first_edict, spawn, start_frame, worldspawn};
use super::{Arg, Case, Check, Input, Program, RETURN, Val, Want, field, global, scenario};

pub(super) fn all() -> Vec<Case> {
    vec![
        unimplemented_builtin(),
        pure_builtins(),
        strings_of_floats(),
        long_joined_text(),
        random_numbers(),
        model_bounds(),
        commands_never_run(),
        command_buffer_full(),
        datagram_overflow(),
        reliable_overflow(),
        signon_overflow(),
        message_to_one_client(),
        message_to_one_non_client(),
        sound_dropped_when_full(),
        invalid_entity(),
        invalid_string(),
        invalid_field(),
        trace_flag(),
        printing_builtins(),
        traceline(),
        pointcontents(),
        checkclient(),
        checkbottom_and_aim(),
        droptofloor(),
        walkmove(),
        walkmove_without_flags(),
        movetogoal(),
        movetogoal_turned_away(),
        movetogoal_without_flags(),
    ]
}

/// `out = builtin(args)`, a float, for a builtin numbered `number`.
fn call_into(p: &mut Program, name: &str, number: i32, args: &[Arg], out: u32) -> Vec<String> {
    let builtin = p.builtin(name, number);
    let mut code = p.call(builtin, args);
    code.push(format!("STORE_F {RETURN} {out}"));
    code
}

/// `body` run `times` times, counting in a new global.
fn repeat(p: &mut Program, body: Vec<String>, times: f32) -> Vec<String> {
    let counter = p.temp();
    let zero = p.float(0.0);
    let one = p.float(1.0);
    let limit = p.float(times);
    let below = p.temp();
    let back = i32::try_from(body.len())
        .unwrap_or(i32::MAX)
        .saturating_add(2)
        .saturating_neg();
    let mut code = vec![format!("STORE_F {zero} {counter}")];
    code.extend(body);
    code.push(format!("ADD_F {counter} {one} {counter}"));
    code.push(format!("LT {counter} {limit} {below}"));
    code.push(format!("IF {below} {back}"));
    code
}

/// A scenario whose `StartFrame` writes `bytes` bytes to message destination `dest` in one
/// frame, with `msg_entity` the first edict, a client, when `to_client` is set, and the world
/// otherwise.
fn writes_in_one_frame(name: &str, dest: f32, bytes: f32, to_client: bool) -> Case {
    let mut p = Program::new();
    let write_byte = p.builtin("WriteByte", 52);
    let dest = p.float(dest);
    let one = p.float(1.0);
    let mut code = if to_client {
        first_edict(&mut p, global::MSG_ENTITY)
    } else {
        Vec::new()
    };
    let body = p.call(write_byte, &[Arg::F(dest), Arg::F(one)]);
    code.extend(repeat(&mut p, body, bytes));
    start_frame(&mut p, code);
    Case::new(name, p, scenario("frames 1\n"))
}

// spec DIFF-6.1: a number with no builtin fails as BUILTIN-1.2 says.
fn unimplemented_builtin() -> Case {
    let mut p = Program::new();
    let missing = p.builtin("missing", 5);
    let code = p.call(missing, &[]);
    worldspawn(&mut p, code);
    Case::new("unimplemented-builtin", p, scenario(""))
        .check(Check::Last(Want::ErrorMessage("unimplemented bulitin")))
}

// spec DIFF-6.1: the class pure builtins behave as builtins.md says, with the step's change
// set holding what they return.
fn pure_builtins() -> Case {
    let mut p = Program::new();
    let v = p.vector([3.0, 4.0, 0.0]);
    let minus = p.float(-2.5);
    let length = p.temp();
    let absolute = p.temp();
    let floor = p.temp();
    let mut code = call_into(&mut p, "vlen", 12, &[Arg::V(v)], length);
    code.extend(call_into(&mut p, "fabs", 43, &[Arg::F(minus)], absolute));
    code.extend(call_into(&mut p, "floor", 37, &[Arg::F(minus)], floor));
    worldspawn(&mut p, code);
    Case::new("pure-builtins", p, scenario("frames 0\n"))
        .check(Check::Seq(vec![
            Want::All(vec![Want::Builtin(12), Want::Global(RETURN, f(5.0))]),
            Want::All(vec![Want::Builtin(43), Want::Global(RETURN, f(2.5))]),
            Want::All(vec![Want::Builtin(37), Want::Global(RETURN, f(-3.0))]),
        ]))
        .check(Check::FinalGlobal(length, f(5.0)))
        .check(Check::FinalGlobal(absolute, f(2.5)))
        .check(Check::FinalGlobal(floor, f(-3.0)))
}

// spec DIFF-6.1: `ftos` and `vtos` write the one temporary string, an engine string whose
// record repeats when its text changes, and print a NaN and an infinity as every C library
// would (BUILTIN-7.6, BUILTIN-7.7).
fn strings_of_floats() -> Case {
    let mut p = Program::new();
    let half = p.float(1.5);
    let whole = p.float(-7.0);
    let nan = p.int(0x7fc0_0000);
    let v = p.temp();
    p.set(v, 0x7fc0_0000);
    let inf = p.temp();
    p.set(inf, 0x7f80_0000);
    let minus_inf = p.temp();
    p.set(minus_inf, 0xff80_0000);
    let ftos = p.builtin("ftos", 26);
    let vtos = p.builtin("vtos", 27);
    let mut code = p.call(ftos, &[Arg::F(half)]);
    code.extend(p.call(ftos, &[Arg::F(whole)]));
    code.extend(p.call(ftos, &[Arg::F(nan)]));
    code.extend(p.call(vtos, &[Arg::V(v)]));
    worldspawn(&mut p, code);
    Case::new("strings-of-floats", p, scenario("frames 0\n")).check(Check::Seq(vec![
        Want::String("  1.5"),
        Want::String("-7"),
        Want::String("  nan"),
        Want::String("'  nan   inf  -inf'"),
    ]))
}

// spec DIFF-6.1: the joined text of `bprint` and the other printing builtins is never cut
// short (BUILTIN-7.5): a text of 300 bytes and one more are joined whole into the error
// record.
fn long_joined_text() -> Case {
    let mut p = Program::new();
    let long = p.string(&"x".repeat(300));
    let more = p.string("y");
    let bprint = p.builtin("bprint", 23);
    let error = p.builtin("error", 10);
    let mut code = p.call(bprint, &[Arg::F(long), Arg::F(more)]);
    code.extend(p.call(error, &[Arg::F(long), Arg::F(more)]));
    worldspawn(&mut p, code);
    Case::new("long-joined-text", p, scenario("frames 0\n")).check(Check::Last(Want::ErrorMessage(
        leaked(&format!("{}y", "x".repeat(300))),
    )))
}

// spec DIFF-6.2: `random` is `(rand & 0x7fff) / 0x7fff` from a 32-bit state that starts at the
// seed and steps by 1103515245 and 12345: from seed 1, the four numbers of the spec's table.
fn random_numbers() -> Case {
    let mut p = Program::new();
    let random = p.builtin("random", 7);
    let mut code = Vec::new();
    let mut slots = Vec::new();
    for _ in 0..4 {
        let out = p.temp();
        code.extend(p.call(random, &[]));
        code.push(format!("STORE_F {RETURN} {out}"));
        slots.push(out);
    }
    worldspawn(&mut p, code);
    let mut case = Case::new("random-numbers", p, scenario("frames 0\n"));
    for (slot, bits) in slots
        .into_iter()
        .zip([0x3f03_8d07, 0x3e33_f168, 0x3e9e_053c, 0x3f08_d712])
    {
        case = case.check(Check::FinalGlobal(slot, Val::Bits(bits)));
    }
    case
}

// spec DIFF-6.3: `setmodel` takes a model directive's bounds, '-16 -16 -16' to '16 16 16' for
// any other name ending in `.mdl`, and zero for everything else, the world and submodels
// included; precaching never fails.
fn model_bounds() -> Case {
    let mut p = Program::new();
    let precache = p.builtin("precache_model", 20);
    let setmodel = p.builtin("setmodel", 3);
    let mut code = Vec::new();
    for name in ["progs/given.mdl", "progs/other.mdl", "progs/s.spr", "*1"] {
        let e = p.temp();
        let text = p.string(name);
        code.extend(p.call(precache, &[Arg::F(text)]));
        code.extend(spawn(&mut p, e));
        code.extend(p.call(setmodel, &[Arg::F(e), Arg::F(text)]));
    }
    worldspawn(&mut p, code);
    let mut case = Case::new(
        "model-bounds",
        p,
        scenario("submodels 1\nmodel progs/given.mdl -1 -2 -3 4 5 6\nframes 0\n"),
    );
    for (axis, (min, max)) in [(-1.0, 4.0), (-2.0, 5.0), (-3.0, 6.0)]
        .into_iter()
        .enumerate()
    {
        let axis = u32::try_from(axis).unwrap_or_default();
        case = case
            .check(Check::FinalField(
                2,
                field::MINS.saturating_add(axis),
                f(min),
            ))
            .check(Check::FinalField(
                2,
                field::MAXS.saturating_add(axis),
                f(max),
            ))
            .check(Check::FinalField(
                3,
                field::MINS.saturating_add(axis),
                f(-16.0),
            ))
            .check(Check::FinalField(
                3,
                field::MAXS.saturating_add(axis),
                f(16.0),
            ))
            .check(Check::FinalField(
                4,
                field::SIZE.saturating_add(axis),
                f(0.0),
            ))
            .check(Check::FinalField(
                5,
                field::SIZE.saturating_add(axis),
                f(0.0),
            ));
    }
    case.check(Check::FinalField(2, field::SIZE, f(5.0)))
}

// spec DIFF-6.4: `localcmd` and `changelevel` run no command and the map never changes: the
// scenario runs every frame on the same map.
fn commands_never_run() -> Case {
    let mut p = Program::new();
    let localcmd = p.builtin("localcmd", 46);
    let changelevel = p.builtin("changelevel", 70);
    let quit = p.string("quit\n");
    let next = p.string("e1m2");
    let mut code = p.call(localcmd, &[Arg::F(quit)]);
    code.extend(p.call(changelevel, &[Arg::F(next)]));
    code.extend(p.call(changelevel, &[Arg::F(next)]));
    let start = start_frame(&mut p, code);
    Case::new("commands-never-run", p, scenario("frames 3\n"))
        .check(Check::Count(Want::Run(start), 5))
        .check(Check::Count(Want::Builtin(70), 10))
        .check(Check::FinalGlobal(global::MAPNAME, Val::Text("test")))
}

// spec DIFF-6.4: the command buffer only counts what is appended; past its 8192 bytes a
// command is dropped, which is no error.
fn command_buffer_full() -> Case {
    let mut p = Program::new();
    let localcmd = p.builtin("localcmd", 46);
    let text = p.string(&"c".repeat(100));
    let body = p.call(localcmd, &[Arg::F(text)]);
    let code = repeat(&mut p, body, 100.0);
    worldspawn(&mut p, code);
    Case::new("command-buffer-full", p, scenario("frames 1\n"))
        .check(Check::Count(Want::Builtin(46), 100))
        .check(Check::Last(Want::Exit))
}

// spec DIFF-6.5: the unreliable datagram holds LIMIT_MAX_DATAGRAM (1024) bytes; one more
// written in a frame is id's fatal overflow error.
fn datagram_overflow() -> Case {
    writes_in_one_frame("datagram-overflow", 0.0, 1025.0, true).check(Check::Last(
        Want::ErrorMessage("SZ_GetSpace: overflow without allowoverflow set"),
    ))
}

// spec DIFF-6.5: so does the reliable datagram (MSG_ALL); 1024 bytes fit.
fn reliable_overflow() -> Case {
    writes_in_one_frame("reliable-overflow", 2.0, 1025.0, true).check(Check::Last(
        Want::ErrorMessage("SZ_GetSpace: overflow without allowoverflow set"),
    ))
}

// spec DIFF-6.5: the signon buffer (MSG_INIT) holds BUILTIN_SIGNON_SIZE (8192) bytes.
fn signon_overflow() -> Case {
    writes_in_one_frame("signon-overflow", 3.0, 8193.0, true).check(Check::Last(
        Want::ErrorMessage("SZ_GetSpace: overflow without allowoverflow set"),
    ))
}

// spec DIFF-6.5: a client's reliable message (MSG_ONE) is not kept and never overflows.
fn message_to_one_client() -> Case {
    writes_in_one_frame("message-to-one", 1.0, 2000.0, true)
        .check(Check::Never(Want::ErrorMessage(
            "SZ_GetSpace: overflow without allowoverflow set",
        )))
        .check(Check::Last(Want::Exit))
}

// spec DIFF-6.5: MSG_ONE to an edict that is no client slot is id's `WriteDest: not a client`.
fn message_to_one_non_client() -> Case {
    writes_in_one_frame("message-to-non-client", 1.0, 1.0, false)
        .check(Check::Last(Want::ErrorMessage("WriteDest: not a client")))
}

// spec DIFF-6.5: a sound or particle that would not fit in the unreliable datagram is
// dropped, as id's server drops it, with no error.
fn sound_dropped_when_full() -> Case {
    let mut p = Program::new();
    let write_byte = p.builtin("WriteByte", 52);
    let sound = p.builtin("sound", 8);
    let particle = p.builtin("particle", 48);
    let precache = p.builtin("precache_sound", 19);
    let zero = p.float(0.0);
    let one = p.float(1.0);
    let sample = p.string("misc/x.wav");
    let origin = p.vector([0.0, 0.0, 0.0]);
    let mut code = p.call(precache, &[Arg::F(sample)]);
    let body = p.call(write_byte, &[Arg::F(zero), Arg::F(one)]);
    code.extend(repeat(&mut p, body, 1010.0));
    for _ in 0..3 {
        code.extend(p.call(
            sound,
            &[
                Arg::F(global::WORLD),
                Arg::F(zero),
                Arg::F(sample),
                Arg::F(one),
                Arg::F(one),
            ],
        ));
        code.extend(p.call(
            particle,
            &[Arg::V(origin), Arg::V(origin), Arg::F(zero), Arg::F(one)],
        ));
    }
    // In the map's spawn, so that the settle frames, which empty no buffer, add nothing.
    worldspawn(&mut p, code);
    Case::new("sound-dropped", p, scenario("frames 1\n"))
        .check(Check::Count(Want::Builtin(8), 3))
        .check(Check::Last(Want::Exit))
}

// spec DIFF-6.6: an entity value that names no edict is the runtime error `invalid entity`.
fn invalid_entity() -> Case {
    let mut p = Program::new();
    let bad = p.int(3);
    let out = p.temp();
    let code = call_into(&mut p, "nextent", 47, &[Arg::F(bad)], out);
    worldspawn(&mut p, code);
    Case::new("invalid-entity", p, scenario(""))
        .check(Check::Last(Want::ErrorMessage("invalid entity")))
}

// spec DIFF-6.6: a string value outside the table that is no engine string is `invalid
// string`.
fn invalid_string() -> Case {
    let mut p = Program::new();
    let bad = p.int(0x0100_0000);
    let bprint = p.builtin("bprint", 23);
    let code = p.call(bprint, &[Arg::F(bad)]);
    worldspawn(&mut p, code);
    Case::new("invalid-string", p, scenario(""))
        .check(Check::Last(Want::ErrorMessage("invalid string")))
}

// spec DIFF-6.6: a field index outside the progs's fields is `invalid field`.
fn invalid_field() -> Case {
    let mut p = Program::new();
    let bad_field = p.int(500);
    let text = p.string("x");
    let out = p.temp();
    let code = call_into(
        &mut p,
        "find",
        18,
        &[Arg::F(global::WORLD), Arg::F(bad_field), Arg::F(text)],
        out,
    );
    worldspawn(&mut p, code);
    Case::new("invalid-field", p, scenario(""))
        .check(Check::Last(Want::ErrorMessage("invalid field")))
}

// spec DIFF-6.7: `traceon` and `traceoff` change only the trace flag, which changes nothing in
// the trace: every statement after them still has its record.
fn trace_flag() -> Case {
    let mut p = Program::new();
    let traceon = p.builtin("traceon", 29);
    let traceoff = p.builtin("traceoff", 30);
    let out = p.temp();
    let one = p.float(1.0);
    let mut code = p.call(traceon, &[]);
    code.push(format!("STORE_F {one} {out}"));
    code.extend(p.call(traceoff, &[]));
    worldspawn(&mut p, code);
    Case::new("trace-flag", p, scenario("frames 0\n")).check(Check::Seq(vec![
        Want::Builtin(29),
        Want::Global(out, f(1.0)),
        Want::Builtin(30),
        Want::Exit,
    ]))
}

// spec DIFF-6.1, DIFF-6.5: the printing and debugging builtins only print, which is not
// compared: `dprint`, `eprint`, `coredump`, `sprint`, `centerprint` and `stuffcmd` to a client.
fn printing_builtins() -> Case {
    let mut p = Program::new();
    let e = p.temp();
    let text = p.string("text\n");
    let mut code = first_edict(&mut p, e);
    for (name, number, args) in [
        ("dprint", 25, vec![Arg::F(text)]),
        ("eprint", 31, vec![Arg::F(e)]),
        ("coredump", 28, Vec::new()),
        ("sprint", 24, vec![Arg::F(e), Arg::F(text)]),
        ("centerprint", 73, vec![Arg::F(e), Arg::F(text)]),
        ("stuffcmd", 21, vec![Arg::F(e), Arg::F(text)]),
    ] {
        let builtin = p.builtin(name, number);
        code.extend(p.call(builtin, &args));
    }
    start_frame(&mut p, code);
    Case::new("printing-builtins", p, scenario("clients 1\nframes 1\n"))
        .check(Check::Count(Want::Builtin(21), 3))
        .check(Check::Last(Want::Exit))
}

// spec DIFF-7.1: `traceline` hits nothing: it writes the nine trace globals, `trace_endpos` a
// copy of its second argument, and nothing else.
fn traceline() -> Case {
    let mut p = Program::new();
    for slot in [
        global::TRACE_ALLSOLID,
        global::TRACE_STARTSOLID,
        global::TRACE_PLANE_DIST,
        global::TRACE_INWATER,
    ] {
        p.set(slot, 5.0_f32.to_bits());
    }
    for slot in global::TRACE_PLANE_NORMAL..global::TRACE_PLANE_NORMAL.saturating_add(3) {
        p.set(slot, 5.0_f32.to_bits());
    }
    let e = p.temp();
    let start = p.vector([1.0, 2.0, 3.0]);
    let end = p.vector([4.0, 5.0, 6.0]);
    let zero = p.float(0.0);
    let traceline = p.builtin("traceline", 16);
    let mut code = first_edict(&mut p, e);
    code.push(format!("STORE_ENT {e} {}", global::TRACE_ENT));
    code.extend(p.call(
        traceline,
        &[Arg::V(start), Arg::V(end), Arg::F(zero), Arg::F(e)],
    ));
    worldspawn(&mut p, code);
    let mut case = Case::new("traceline", p, scenario("frames 0\n"));
    for (slot, value) in [
        (global::TRACE_ALLSOLID, 0.0),
        (global::TRACE_STARTSOLID, 0.0),
        (global::TRACE_FRACTION, 1.0),
        (global::TRACE_ENDPOS, 4.0),
        (global::TRACE_ENDPOS.saturating_add(1), 5.0),
        (global::TRACE_ENDPOS.saturating_add(2), 6.0),
        (global::TRACE_PLANE_NORMAL, 0.0),
        (global::TRACE_PLANE_NORMAL.saturating_add(1), 0.0),
        (global::TRACE_PLANE_NORMAL.saturating_add(2), 0.0),
        (global::TRACE_PLANE_DIST, 0.0),
        (global::TRACE_INOPEN, 1.0),
        (global::TRACE_INWATER, 0.0),
    ] {
        case = case.check(Check::FinalGlobal(slot, f(value)));
    }
    case.check(Check::FinalGlobal(global::TRACE_ENT, Val::Entity(0)))
}

// spec DIFF-7.2: `pointcontents` returns -1, CONTENTS_EMPTY.
fn pointcontents() -> Case {
    let mut p = Program::new();
    let point = p.vector([0.0, 0.0, -10000.0]);
    let out = p.temp();
    let code = call_into(&mut p, "pointcontents", 41, &[Arg::V(point)], out);
    worldspawn(&mut p, code);
    Case::new("pointcontents", p, scenario("frames 0\n")).check(Check::FinalGlobal(out, f(-1.0)))
}

// spec DIFF-7.3: `checkclient` chooses, at most every 0.1 seconds, the next client slot after
// the last choice that is in use, alive and not FL_NOTARGET, keeps the last choice when none
// is, and returns the world when its choice is dead.
fn checkclient() -> Case {
    let mut p = Program::new();
    let mut functions = Vec::new();
    let mut outs = Vec::new();
    for name in ["check_a", "check_b", "check_c"] {
        let out = p.temp();
        let code = call_into(&mut p, "checkclient", 17, &[], out);
        functions.push(p.func(name, &done(code)));
        outs.push(out);
    }
    let zero = p.float(0.0);
    let e1 = p.temp();
    let e3 = p.temp();
    let out = p.temp();
    let mut code = p.write_field(e1, field::HEALTH, zero);
    code.extend(p.write_field(e3, field::HEALTH, zero));
    code.extend(call_into(&mut p, "checkclient", 17, &[], out));
    let last = p.func("check_dead", &done(code));
    let hundred = p.float(100.0);
    let revived_out = p.temp();
    let mut code = p.write_field(e1, field::HEALTH, hundred);
    code.extend(p.write_field(e3, field::HEALTH, hundred));
    code.extend(call_into(&mut p, "checkclient", 17, &[], revived_out));
    let revived = p.func("check_revived", &done(code));
    let alive = 0x42c8_0000_u32;
    let mut init = format!(
        "edicts 4\nmaxclients 3\nbuiltins engine\nglobal {e1} entity 1\nglobal {e3} entity 3\n\
         field 1 48 bits {alive:08x}\nfield 2 48 bits {alive:08x}\nfield 2 76 bits 43000000\n\
         field 3 48 bits {alive:08x}\n"
    );
    for (clock, function) in [(0.1, 0), (0.15, 1), (0.2, 2)] {
        let index = functions.get(function).copied().unwrap_or_default();
        init.push_str(&format!("host clock {clock}\nhost run {index}\n"));
    }
    init.push_str(&format!(
        "host clock 0.4\nhost run {last}\nhost clock 0.6\nhost run {revived}\n"
    ));
    let [a, b, c] = [0, 1, 2].map(|i| outs.get(i).copied().unwrap_or_default());
    Case::new("checkclient", p, Input::Vector { init })
        // From the last choice 0, clamped to 1: 2 is FL_NOTARGET (128), so 3.
        .check(Check::FinalGlobal(a, Val::Entity(3)))
        // 0.05 seconds later: the same choice.
        .check(Check::FinalGlobal(b, Val::Entity(3)))
        // After 3 comes 1.
        .check(Check::FinalGlobal(c, Val::Entity(1)))
        // 1 and 3 dead, 2 not a target: the choice stays 1, which is dead, so the world.
        .check(Check::FinalGlobal(out, Val::Entity(0)))
        // All alive again: after the kept choice 1 comes 3.
        .check(Check::FinalGlobal(revived_out, Val::Entity(3)))
}

// spec DIFF-7.4: `checkbottom` returns 1; `aim` returns a copy of `v_forward`.
fn checkbottom_and_aim() -> Case {
    let mut p = Program::new();
    for (offset, value) in [0.6_f32, 0.8, 0.0].into_iter().enumerate() {
        let slot = global::V_FORWARD.saturating_add(u32::try_from(offset).unwrap_or_default());
        p.set(slot, value.to_bits());
    }
    let e = p.temp();
    let speed = p.float(1000.0);
    let bottom = p.temp();
    let aimed = p.vector([0.0, 0.0, 0.0]);
    let mut code = first_edict(&mut p, e);
    code.extend(call_into(&mut p, "checkbottom", 40, &[Arg::F(e)], bottom));
    let aim = p.builtin("aim", 44);
    code.extend(p.call(aim, &[Arg::F(e), Arg::F(speed)]));
    code.push(format!("STORE_V {RETURN} {aimed}"));
    worldspawn(&mut p, code);
    Case::new("checkbottom-and-aim", p, scenario("frames 0\n"))
        .check(Check::FinalGlobal(bottom, f(1.0)))
        .check(Check::FinalGlobal(aimed, f(0.6)))
        .check(Check::FinalGlobal(aimed.saturating_add(1), f(0.8)))
}

// spec DIFF-7.5: `droptofloor` leaves `origin`, links `self`, sets FL_ONGROUND (512) in
// `flags`, sets `groundentity` to the world and returns 1.
fn droptofloor() -> Case {
    let mut p = Program::new();
    let e = p.temp();
    let fly = p.float(1.0);
    let origin = p.vector([0.0, 0.0, 100.0]);
    let out = p.temp();
    let mut code = spawn(&mut p, e);
    code.extend(p.write_field(e, field::FLAGS, fly));
    code.extend(p.write_vector_field(e, field::ORIGIN, origin));
    let other = p.temp();
    code.extend(first_edict(&mut p, other));
    code.extend(p.write_field(e, field::GROUNDENTITY, other));
    code.push(format!("STORE_ENT {e} {}", global::SELF));
    code.extend(call_into(&mut p, "droptofloor", 34, &[], out));
    worldspawn(&mut p, code);
    Case::new("droptofloor", p, scenario("frames 0\n"))
        .check(Check::FinalGlobal(out, f(1.0)))
        .check(Check::FinalField(2, field::FLAGS, f(513.0)))
        .check(Check::FinalField(2, field::GROUNDENTITY, Val::Entity(0)))
        .check(Check::FinalField(
            2,
            field::ORIGIN.saturating_add(2),
            f(100.0),
        ))
        .check(Check::FinalField(
            2,
            field::ABSMIN.saturating_add(2),
            f(99.0),
        ))
}

/// A program whose `worldspawn` spawns edict 2 with `flags` and `origin` '10 20 30', makes it
/// `self` and runs `code`, which `make` builds.
fn on_self(flags: f32, make: impl FnOnce(&mut Program) -> Vec<String>) -> Program {
    let mut p = Program::new();
    let e = p.temp();
    let flags = p.float(flags);
    let origin = p.vector([10.0, 20.0, 30.0]);
    let mut code = spawn(&mut p, e);
    code.extend(p.write_field(e, field::FLAGS, flags));
    code.extend(p.write_vector_field(e, field::ORIGIN, origin));
    code.push(format!("STORE_ENT {e} {}", global::SELF));
    code.extend(make(&mut p));
    worldspawn(&mut p, code);
    p
}

// spec DIFF-7.6, DIFF-7.7: `walkmove` of an edict on the ground steps along the yaw by the
// distance, in x and y only, links it and returns 1: yaw 180 by 10 from '10 20 30' is
// '0 20 30' (the sine's residue is below a float's step at 20).
fn walkmove() -> Case {
    let mut out = 0;
    let p = on_self(512.0, |p| {
        let yaw = p.float(180.0);
        let dist = p.float(10.0);
        out = p.temp();
        call_into(p, "walkmove", 32, &[Arg::F(yaw), Arg::F(dist)], out)
    });
    Case::new("walkmove", p, scenario("frames 0\n"))
        .check(Check::FinalGlobal(out, f(1.0)))
        .check(Check::FinalField(2, field::ORIGIN, f(0.0)))
        .check(Check::FinalField(
            2,
            field::ORIGIN.saturating_add(1),
            f(20.0),
        ))
        .check(Check::FinalField(
            2,
            field::ORIGIN.saturating_add(2),
            f(30.0),
        ))
        .check(Check::FinalField(2, field::ABSMIN, f(-1.0)))
}

// spec DIFF-7.7: without FL_ONGROUND, FL_FLY or FL_SWIM, `walkmove` returns 0 and moves
// nothing.
fn walkmove_without_flags() -> Case {
    let mut out = 0;
    let p = on_self(4096.0, |p| {
        let yaw = p.float(0.0);
        let dist = p.float(10.0);
        out = p.temp();
        let one = p.float(1.0);
        let mut code = vec![format!("STORE_F {one} {out}")];
        code.extend(call_into(
            p,
            "walkmove",
            32,
            &[Arg::F(yaw), Arg::F(dist)],
            out,
        ));
        code
    });
    Case::new("walkmove-without-flags", p, scenario("frames 0\n"))
        .check(Check::Seq(vec![
            Want::Global(out, f(1.0)),
            Want::Global(out, f(0.0)),
        ]))
        .check(Check::FinalField(2, field::ORIGIN, f(10.0)))
        .check(Check::Never(Want::FieldChanged(2, field::ABSMIN)))
}

/// `movetogoal(8)` on an edict flying with `angles_y` and `ideal_yaw` given, `yaw_speed` 0;
/// the slot holding what `RETURN` held, 7, before the call.
fn movetogoal_from(angle: f32, ideal: f32, flags: f32) -> (Program, u32) {
    let mut out = 0;
    let p = on_self(flags, |p| {
        let angle = p.float(angle);
        let ideal = p.float(ideal);
        let seven = p.float(7.0);
        let dist = p.float(8.0);
        out = p.temp();
        let mut code = p.write_field(global::SELF, field::ANGLES.saturating_add(1), angle);
        code.extend(p.write_field(global::SELF, field::IDEAL_YAW, ideal));
        code.push(format!("STORE_F {seven} {RETURN}"));
        let movetogoal = p.builtin("movetogoal", 67);
        code.extend(p.call(movetogoal, &[Arg::F(dist)]));
        code.push(format!("STORE_F {RETURN} {out}"));
        code
    });
    (p, out)
}

// spec DIFF-7.8: `movetogoal` of an edict that can move turns it as `changeyaw` does, steps
// along `ideal_yaw` when the turned yaw is within 45 degrees of it, links it and writes
// nothing to RETURN.
fn movetogoal() -> Case {
    let (p, out) = movetogoal_from(0.0, 0.0, 1.0);
    Case::new("movetogoal", p, scenario("frames 0\n"))
        .check(Check::FinalGlobal(out, f(7.0)))
        .check(Check::FinalField(2, field::ORIGIN, f(18.0)))
        .check(Check::FinalField(
            2,
            field::ORIGIN.saturating_add(1),
            f(20.0),
        ))
        .check(Check::FinalField(2, field::ABSMAX, f(19.0)))
}

// spec DIFF-7.8: when the yaw is more than 45 degrees from `ideal_yaw` the step is undone.
fn movetogoal_turned_away() -> Case {
    let (p, out) = movetogoal_from(100.0, 0.0, 2.0);
    Case::new("movetogoal-turned-away", p, scenario("frames 0\n"))
        .check(Check::FinalGlobal(out, f(7.0)))
        .check(Check::FinalField(2, field::ORIGIN, f(10.0)))
        .check(Check::FinalField(2, field::ABSMAX, f(11.0)))
}

// spec DIFF-7.8: an edict that cannot move gets 0 in RETURN and nothing else.
fn movetogoal_without_flags() -> Case {
    let (p, out) = movetogoal_from(0.0, 0.0, 0.0);
    Case::new("movetogoal-without-flags", p, scenario("frames 0\n"))
        .check(Check::FinalGlobal(out, f(0.0)))
        .check(Check::FinalField(2, field::ORIGIN, f(10.0)))
}

/// A text a check expects, made at run time; the cases live for the whole run.
fn leaked(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}
