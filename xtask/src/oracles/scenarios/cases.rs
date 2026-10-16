// SPDX-License-Identifier: GPL-3.0-or-later

//! The scenario tests (format in the parent module). Each function builds one case, or one
//! family of cases, and the line above it cites the statements it pins.

use super::{
    Arg, Case, Check, Input, Program, RETURN, Val, WORLDSPAWN, Want, field, global, raw_scenario,
    scenario, scenario_with_map, ty,
};

/// Every case, in the order `cargo xtask oracles` runs them.
pub(super) fn all() -> Vec<Case> {
    let mut cases = vec![
        whole_scenario(),
        builtin_table(),
        fixed_answers(),
        vector_key_with_two_components(),
        deterministic(),
        nothing_moves(),
        scenario_lines(),
        defaults(),
        default_clients_and_seed(),
        settings(),
        default_settings(),
        entities_file(),
        default_entities(),
        input_lines(),
        engine_state(),
        precache_lists(),
        world_fields(),
        deathmatch_global(),
        spawn_inhibited_by_skill(),
        settle_frames(),
        ends_normally(),
    ];
    cases.extend(skill_is_rounded_and_clamped());
    cases.extend(refusals());
    cases.push(last_line_without_a_line_feed());
    cases.push(relative_scenario_path());
    cases.extend(refused_progs());
    cases.extend(super::frames::all());
    cases.extend(super::world::all());
    cases.extend(super::errors::all());
    cases.extend(super::trace::all());
    cases
}

/// Statements followed by `DONE`.
pub(super) fn done(mut code: Vec<String>) -> Vec<String> {
    code.push("DONE".to_owned());
    code
}

/// Makes `StartFrame` a function running `code`; returns its index.
pub(super) fn start_frame(p: &mut Program, code: Vec<String>) -> i32 {
    let start = p.func("start", &done(code));
    p.system(global::START_FRAME, start);
    start
}

/// Makes the map's `worldspawn` run `code`.
pub(super) fn worldspawn(p: &mut Program, code: Vec<String>) {
    p.body(WORLDSPAWN, &done(code));
}

/// A system function global naming a new empty function, so that its runs can be counted.
pub(super) fn named(p: &mut Program, slot: u32, name: &str) -> i32 {
    let function = p.func(name, &done(Vec::new()));
    p.system(slot, function);
    function
}

/// `out = nextent(world)`: edict 1 when the map has one in use (BUILTIN-8).
pub(super) fn first_edict(p: &mut Program, out: u32) -> Vec<String> {
    let nextent = p.builtin("nextent", 47);
    let mut code = p.call(nextent, &[Arg::F(global::WORLD)]);
    code.push(format!("STORE_ENT {RETURN} {out}"));
    code
}

/// `out = spawn()`.
pub(super) fn spawn(p: &mut Program, out: u32) -> Vec<String> {
    let spawn = p.builtin("spawn", 14);
    let mut code = p.call(spawn, &[]);
    code.push(format!("STORE_ENT {RETURN} {out}"));
    code
}

pub(super) fn f(value: f32) -> Val {
    Val::F(value)
}

/// `remove(entity)`.
pub(super) fn remove(p: &mut Program, entity: u32) -> Vec<String> {
    let remove = p.builtin("remove", 15);
    p.call(remove, &[Arg::F(entity)])
}

// spec DIFF-1.1: the oracle writes one trace of the whole scenario, from the map's spawn
// functions to the last server frame.
fn whole_scenario() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    Case::new("whole-scenario", p, scenario("frames 3\n"))
        .check(Check::Seq(vec![
            Want::Run(WORLDSPAWN),
            Want::Exit,
            Want::Run(start),
            Want::Exit,
        ]))
        .check(Check::Count(Want::Run(WORLDSPAWN), 1))
        .check(Check::Count(Want::Run(start), 5))
        .check(Check::Last(Want::Exit))
}

// spec DIFF-1.2: the mock's builtin table has 79 entries: builtin 78 runs, and a call of 79
// is id's `Bad builtin call number`.
fn builtin_table() -> Case {
    let mut p = Program::new();
    let e = p.temp();
    let setspawnparms = p.builtin("setspawnparms", 78);
    let beyond = p.builtin("beyond", 79);
    let mut code = first_edict(&mut p, e);
    code.extend(p.call(setspawnparms, &[Arg::F(e)]));
    code.extend(p.call(beyond, &[]));
    worldspawn(&mut p, code);
    Case::new("builtin-table", p, scenario(""))
        .check(Check::Seq(vec![
            Want::Builtin(78),
            Want::ErrorMessage("Bad builtin call number"),
        ]))
        .check(Check::Last(Want::ErrorMessage("Bad builtin call number")))
}

// spec DIFF-1.3: where builtins.md fixes an answer id's C leaves undefined, the oracle gives
// the fixed answer: a lightstyle outside 0 to 63 is `invalid lightstyle` (BUILTIN-12.6).
fn fixed_answers() -> Case {
    let mut p = Program::new();
    let lightstyle = p.builtin("lightstyle", 35);
    let style = p.float(64.0);
    let text = p.string("a");
    let code = p.call(lightstyle, &[Arg::F(style), Arg::F(text)]);
    worldspawn(&mut p, code);
    Case::new("fixed-answers", p, scenario(""))
        .check(Check::Last(Want::ErrorMessage("invalid lightstyle")))
}

// spec DIFF-1.3: a vector key with two components spawns with 0 for the third, the answer
// EDICT-8.3 fixes where id's ED_ParseEpair reads past the value.
fn vector_key_with_two_components() -> Case {
    let mut p = Program::new();
    p.field_def("origin", ty::VECTOR, field::ORIGIN);
    let thing = p.func("thing", &done(Vec::new()));
    let map =
        "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"thing\"\n\"origin\" \"1 2\"\n}\n";
    Case::new("vector-key", p, scenario_with_map("", map))
        .check(Check::Count(Want::Run(thing), 1))
        .check(Check::FinalField(2, field::ORIGIN, f(1.0)))
        .check(Check::FinalField(
            2,
            field::ORIGIN.saturating_add(1),
            f(2.0),
        ))
        .check(Check::FinalField(
            2,
            field::ORIGIN.saturating_add(2),
            f(0.0),
        ))
}

// spec DIFF-1.4: running one scenario twice writes the same bytes, random numbers included.
fn deterministic() -> Case {
    let mut p = Program::new();
    let random = p.builtin("random", 7);
    let out = p.temp();
    let mut code = p.call(random, &[]);
    code.push(format!("STORE_F {RETURN} {out}"));
    start_frame(&mut p, code);
    Case::new("deterministic", p, scenario("frames 5\nseed 99\n"))
        .check(Check::SameTwice)
        .check(Check::Count(Want::GlobalChanged(out), 7))
}

// spec DIFF-1.5, DIFF-5.8: nothing moves and nothing collides: an entity with a velocity keeps
// its origin, and its `touch` never runs.
fn nothing_moves() -> Case {
    let mut p = Program::new();
    let touch = p.func("touched", &done(Vec::new()));
    let e = p.temp();
    let toss = p.float(6.0);
    let velocity = p.vector([100.0, 50.0, -800.0]);
    let touch_ref = p.fref(touch);
    let mut code = spawn(&mut p, e);
    code.extend(p.write_field(e, field::MOVETYPE, toss));
    code.extend(p.write_vector_field(e, field::VELOCITY, velocity));
    code.extend(p.write_field(e, field::TOUCH, touch_ref));
    worldspawn(&mut p, code);
    Case::new("nothing-moves", p, scenario("frames 20\n"))
        .check(Check::FinalField(2, field::VELOCITY, f(100.0)))
        .check(Check::Never(Want::FieldChanged(2, field::ORIGIN)))
        .check(Check::Never(Want::FieldChanged(
            2,
            field::ORIGIN.saturating_add(2),
        )))
        .check(Check::Never(Want::Run(touch)))
}

// spec DIFF-2.1: CR LF line ends, comments, blank lines of spaces and tabs, words separated
// by runs of spaces and tabs, and a path read relative to the scenario's directory, not the
// oracle's working directory.
fn scenario_lines() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    let thing = p.func("thing", &done(Vec::new()));
    let text = b"# a comment line\r\nprogs\tprogs.dat\r\n \t \r\n\r\nframes  \t 2 # two frames\r\n\
                 entities maps/sub.ent\r\n";
    let map = b"{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"thing\"\n}\n";
    Case::new(
        "scenario-lines",
        p,
        raw_scenario(text, &[("maps/sub.ent", map)]),
    )
    .check(Check::Count(Want::Run(start), 4))
    .check(Check::Count(Want::Run(thing), 1))
}

// spec DIFF-2.2: with only `progs`, the scenario runs 100 frames of 0.1 seconds on the map
// `test`.
fn defaults() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    Case::new("defaults", p, scenario(""))
        .check(Check::Count(Want::Run(start), 102))
        .check(Check::FinalGlobal(global::FRAMETIME, f(0.1)))
        .check(Check::FinalGlobal(global::MAPNAME, Val::Text("test")))
}

// spec DIFF-2.2: by default one client slot, no client enters, there are no submodels, and
// the random state is 1.
fn default_clients_and_seed() -> Case {
    let mut p = Program::new();
    let connect = named(&mut p, global::CLIENT_CONNECT, "connect");
    let random = p.builtin("random", 7);
    let first = p.temp();
    let e = p.temp();
    let mut code = p.call(random, &[]);
    code.push(format!("STORE_F {RETURN} {first}"));
    code.extend(spawn(&mut p, e));
    let setmodel = p.builtin("setmodel", 3);
    let submodel = p.string("*1");
    code.extend(p.call(setmodel, &[Arg::F(e), Arg::F(submodel)]));
    worldspawn(&mut p, code);
    Case::new("default-clients-and-seed", p, scenario(""))
        // DIFF-6.2: the first random number from state 1.
        .check(Check::Seq(vec![
            Want::Global(first, Val::Bits(0x3f03_8d07)),
            // maxclients 1: the first free edict is 2.
            Want::Global(RETURN, Val::Entity(2)),
            Want::ErrorMessage("no precache: *1"),
        ]))
        .check(Check::Never(Want::Run(connect)))
}

/// `out = cvar(name)` for each name, in order, then `cvar_set("unset", "3")` and
/// `cvar("unset")` again; returns the code and the slots, in the same order, the last one
/// the value after `cvar_set`.
fn read_settings(p: &mut Program, names: &[&str]) -> (Vec<String>, Vec<u32>) {
    let cvar = p.builtin("cvar", 45);
    let cvar_set = p.builtin("cvar_set", 72);
    let mut code = Vec::new();
    let mut slots = Vec::new();
    for name in names {
        let text = p.string(name);
        let out = p.temp();
        code.extend(p.call(cvar, &[Arg::F(text)]));
        code.push(format!("STORE_F {RETURN} {out}"));
        slots.push(out);
    }
    let unset = p.string("unset");
    let three = p.string("3");
    code.extend(p.call(cvar_set, &[Arg::F(unset), Arg::F(three)]));
    let out = p.temp();
    code.extend(p.call(cvar, &[Arg::F(unset)]));
    code.push(format!("STORE_F {RETURN} {out}"));
    slots.push(out);
    (code, slots)
}

// spec DIFF-2.4: a `cvar` line sets one of the four settings or adds one, a later line for a
// name replaces an earlier one, a value is its text read by `Q_atof`, and any other name is 0
// whatever `cvar_set` does.
fn settings() -> Case {
    let mut p = Program::new();
    let (code, slots) = read_settings(
        &mut p,
        &["skill", "teamplay", "added", "hex", "later", "unset"],
    );
    worldspawn(&mut p, code);
    let mut case = Case::new(
        "settings",
        p,
        scenario(
            "cvar skill 2\ncvar teamplay 1\ncvar added 7\ncvar hex 0x10\ncvar later 1\n\
             cvar later 5.5\n",
        ),
    );
    for (slot, value) in slots.into_iter().zip([2.0, 1.0, 7.0, 16.0, 5.5, 0.0, 0.0]) {
        case = case.check(Check::FinalGlobal(slot, f(value)));
    }
    case
}

// spec DIFF-2.4: the four settings start as `1`, `0`, `0` and `0`.
fn default_settings() -> Case {
    let mut p = Program::new();
    let (code, slots) = read_settings(&mut p, &["skill", "deathmatch", "coop", "teamplay"]);
    worldspawn(&mut p, code);
    let mut case = Case::new("default-settings", p, scenario(""));
    for (slot, value) in slots.into_iter().zip([1.0, 0.0, 0.0, 0.0, 0.0]) {
        case = case.check(Check::FinalGlobal(slot, f(value)));
    }
    case
}

// spec DIFF-2.5: the entities file is read as ED_LoadFromFile reads a BSP's entity lump: each
// entity runs its spawn function, in file order, with `self` the new edict.
fn entities_file() -> Case {
    let mut p = Program::new();
    p.field_def("message", ty::STRING, field::MESSAGE);
    let first = p.func("first", &done(Vec::new()));
    let second = p.func("second", &done(Vec::new()));
    let map = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"first\"\n\
               \"message\" \"one\"\n}\n{\n\"classname\" \"second\"\n}\n";
    Case::new("entities-file", p, scenario_with_map("frames 0\n", map))
        .check(Check::Seq(vec![
            Want::Run(WORLDSPAWN),
            Want::All(vec![
                Want::Run(first),
                Want::Global(global::SELF, Val::Entity(2)),
            ]),
            Want::All(vec![
                Want::Run(second),
                Want::Global(global::SELF, Val::Entity(3)),
            ]),
        ]))
        .check(Check::FinalField(2, field::MESSAGE, Val::EngineText("one")))
}

// spec DIFF-2.5: with no `entities` line the map is the one entity `worldspawn`.
fn default_entities() -> Case {
    let p = Program::new();
    Case::new("default-entities", p, scenario("frames 0\n"))
        .check(Check::Count(Want::Run(WORLDSPAWN), 1))
        .check(Check::Count(Want::Run(super::NOOP), 2))
}

// spec DIFF-2.6, DIFF-4.3: each `input` line sets its field of its client's edict at the start
// of its frame, before `StartFrame`, in file order.
fn input_lines() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    Case::new(
        "input-lines",
        p,
        scenario(
            "maxclients 2\nclients 2\nframes 3\ninput 2 1 impulse 9\ninput 2 2 button0 1\n\
             input 3 1 v_angle 1 -2 3.5\ninput 3 2 button2 1\ninput 3 2 button0 0\n\
             input 3 1 impulse 255\n",
        ),
    )
    .check(Check::Seq(vec![
        // Two settle frames and frame 1 first.
        Want::Run(start),
        Want::Run(start),
        Want::Run(start),
        Want::All(vec![
            Want::Run(start),
            Want::Field(1, field::IMPULSE, f(9.0)),
            Want::Field(2, field::BUTTON0, f(1.0)),
        ]),
        Want::All(vec![
            Want::Run(start),
            Want::Field(1, field::V_ANGLE, f(1.0)),
            Want::Field(1, field::V_ANGLE.saturating_add(1), f(-2.0)),
            Want::Field(1, field::V_ANGLE.saturating_add(2), f(3.5)),
            Want::Field(2, field::BUTTON2, f(1.0)),
            Want::Field(2, field::BUTTON0, f(0.0)),
            Want::Field(1, field::IMPULSE, f(255.0)),
        ]),
    ]))
}

// spec DIFF-3.2: `deathmatch` becomes 0 when `coop` is not, the clock is 1.0 when the map
// spawns, `num_edicts` is `maxclients + 1`, and the random state is the seed.
fn engine_state() -> Case {
    let mut p = Program::new();
    let (mut code, slots) = read_settings(&mut p, &["deathmatch", "coop"]);
    let e = p.temp();
    let random = p.builtin("random", 7);
    let number = p.temp();
    code.extend(spawn(&mut p, e));
    code.extend(p.call(random, &[]));
    code.push(format!("STORE_F {RETURN} {number}"));
    worldspawn(&mut p, code);
    let deathmatch = slots.first().copied().unwrap_or_default();
    let coop = slots.get(1).copied().unwrap_or_default();
    Case::new(
        "engine-state",
        p,
        scenario("cvar coop 1\ncvar deathmatch 3\nmaxclients 4\nseed 2\nframes 0\n"),
    )
    .check(Check::Seq(vec![Want::All(vec![
        Want::Run(WORLDSPAWN),
        Want::Global(global::TIME, f(1.0)),
    ])]))
    .check(Check::FinalGlobal(deathmatch, f(0.0)))
    .check(Check::FinalGlobal(coop, f(1.0)))
    .check(Check::FinalGlobal(e, Val::Entity(5)))
    // DIFF-6.2 from state 2: 2 * 1103515245 + 12345 = 2207042835, whose bits 16 and up are
    // 33676; and 0x7fff leaves 908.
    .check(Check::FinalGlobal(number, f(908.0 / 32767.0)))
}

// spec DIFF-3.2: the skill is `(int)(skill + 0.5)` clamped to 0 to 3, and becomes the value of
// `skill`.
fn skill_is_rounded_and_clamped() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, text, value) in [
        ("skill-rounded", "2.6", 3.0),
        ("skill-below-0", "-1", 0.0),
        ("skill-above-3", "7", 3.0),
    ] {
        let mut p = Program::new();
        let (code, slots) = read_settings(&mut p, &["skill"]);
        worldspawn(&mut p, code);
        let skill = slots.first().copied().unwrap_or_default();
        cases.push(
            Case::new(name, p, scenario(&format!("cvar skill {text}\nframes 0\n")))
                .check(Check::FinalGlobal(skill, f(value))),
        );
    }
    cases
}

// spec DIFF-3.3: the model precache list is the empty name, `maps/<map>.bsp` and the
// submodels `*1` to `*n`, so the next model precached is entry n + 2.
fn precache_lists() -> Case {
    let mut p = Program::new();
    let setmodel = p.builtin("setmodel", 3);
    let precache = p.builtin("precache_model", 20);
    let e = p.temp();
    let world_model = p.string("maps/start.bsp");
    let second = p.string("*2");
    let mdl = p.string("progs/x.mdl");
    let index = p.temp();
    let mut code = spawn(&mut p, e);
    code.extend(p.call(setmodel, &[Arg::F(e), Arg::F(world_model)]));
    code.extend(p.read_field(e, field::MODELINDEX, index));
    code.extend(p.call(setmodel, &[Arg::F(e), Arg::F(second)]));
    code.extend(p.read_field(e, field::MODELINDEX, index));
    code.extend(p.call(precache, &[Arg::F(mdl)]));
    code.extend(p.call(setmodel, &[Arg::F(e), Arg::F(mdl)]));
    code.extend(p.read_field(e, field::MODELINDEX, index));
    worldspawn(&mut p, code);
    Case::new(
        "precache-lists",
        p,
        scenario("map start\nsubmodels 2\nframes 0\n"),
    )
    .check(Check::Seq(vec![
        Want::Global(index, f(1.0)),
        Want::Global(index, f(3.0)),
        Want::Global(index, f(4.0)),
    ]))
}

// spec DIFF-3.4, DIFF-3.8, TRACE-4.10: before the first run the world gets its fields and the
// globals `mapname` and `time`; `model` and `mapname` are engine strings, values outside the
// string table whose string records come before the run record that shows them.
fn world_fields() -> Case {
    let p = Program::new();
    Case::new("world-fields", p, scenario("map e1m1\nframes 0\n"))
        .check(Check::Seq(vec![
            Want::String("maps/e1m1.bsp"),
            Want::String("e1m1"),
            Want::All(vec![
                Want::Run(WORLDSPAWN),
                Want::Field(0, field::MODEL, Val::EngineText("maps/e1m1.bsp")),
                Want::Field(0, field::MODELINDEX, f(1.0)),
                Want::Field(0, field::SOLID, f(4.0)),
                Want::Field(0, field::MOVETYPE, f(7.0)),
                Want::Global(global::MAPNAME, Val::EngineText("e1m1")),
                Want::Global(global::TIME, f(1.0)),
            ]),
        ]))
        .check(Check::Never(Want::GlobalChanged(global::SERVERFLAGS)))
        .check(Check::Never(Want::GlobalChanged(global::DEATHMATCH)))
}

// spec DIFF-3.4: without `coop` the global `deathmatch` gets the setting's value.
fn deathmatch_global() -> Case {
    let p = Program::new();
    Case::new(
        "deathmatch-global",
        p,
        scenario("cvar deathmatch 2\nframes 0\n"),
    )
    .check(Check::Seq(vec![Want::All(vec![
        Want::Run(WORLDSPAWN),
        Want::Global(global::DEATHMATCH, f(2.0)),
    ])]))
    .check(Check::Never(Want::GlobalChanged(global::COOP)))
}

// spec DIFF-3.5: the map's entities spawn by EDICT-8 with the skill of DIFF-3.2: on skill 0 an
// entity flagged not-on-easy (256) is removed before its spawn function runs.
fn spawn_inhibited_by_skill() -> Case {
    let mut p = Program::new();
    let thing = p.func("thing", &done(Vec::new()));
    let map = "{\n\"classname\" \"worldspawn\"\n}\n{\n\"classname\" \"thing\"\n\
               \"spawnflags\" \"256\"\n}\n{\n\"classname\" \"thing\"\n\"spawnflags\" \"512\"\n}\n";
    Case::new(
        "spawn-inhibited",
        p,
        scenario_with_map("cvar skill 0.4\nframes 0\n", map),
    )
    .check(Check::Count(Want::Run(thing), 1))
}

// spec DIFF-3.6, DIFF-5.2: two settle frames of 0.1 seconds that do not set `frametime`, then
// the clients enter, then the frames, the first at clock 1.2000000000000002.
fn settle_frames() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    let new_parms = named(&mut p, global::SET_NEW_PARMS, "new_parms");
    let connect = named(&mut p, global::CLIENT_CONNECT, "connect");
    let put = named(&mut p, global::PUT_CLIENT_IN_SERVER, "put");
    let mut clock = 1.0_f64;
    let mut times = Vec::new();
    for _ in 0..2 {
        times.push(clock);
        clock += 0.1;
    }
    for _ in 0..2 {
        times.push(clock);
        clock += 0.05;
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "spec DIFF-5.3: time is (f32)clock, C's rounding of a double to a float"
    )]
    let time = |seconds: f64| f(seconds as f32);
    Case::new(
        "settle-frames",
        p,
        scenario("clients 1\nframes 2\nframetime 0.05\n"),
    )
    .check(Check::Seq(vec![
        Want::Run(WORLDSPAWN),
        // The first settle frame's time is the 1.0 the map spawned at: no change.
        Want::Run(start),
        Want::All(vec![
            Want::Run(start),
            Want::Global(
                global::TIME,
                time(times.get(1).copied().unwrap_or_default()),
            ),
        ]),
        Want::Run(new_parms),
        // The client enters at the clock after the settle frames (DIFF-4.1), so frame 1's
        // time is already set.
        Want::All(vec![
            Want::Run(connect),
            Want::Global(global::TIME, Val::Bits(0x3f99_999a)),
        ]),
        Want::Run(put),
        Want::All(vec![
            Want::Run(start),
            Want::Global(global::FRAMETIME, f(0.05)),
        ]),
        Want::All(vec![
            Want::Run(start),
            Want::Global(
                global::TIME,
                time(times.get(3).copied().unwrap_or_default()),
            ),
        ]),
    ]))
    .check(Check::Count(Want::GlobalChanged(global::FRAMETIME), 1))
}

// spec DIFF-3.7: a scenario that ends normally runs no `ClientDisconnect` and no
// `SetChangeParms`.
fn ends_normally() -> Case {
    let mut p = Program::new();
    let disconnect = named(&mut p, global::CLIENT_DISCONNECT, "disconnect");
    let change = named(&mut p, global::SET_CHANGE_PARMS, "change_parms");
    let post = named(&mut p, global::PLAYER_POST_THINK, "post");
    Case::new("ends-normally", p, scenario("clients 1\nframes 2\n"))
        .check(Check::Count(Want::Run(post), 2))
        .check(Check::Never(Want::Run(disconnect)))
        .check(Check::Never(Want::Run(change)))
        .check(Check::Last(Want::Exit))
}

// spec DIFF-2.9: a scenario with an unknown key, a missing or extra argument, a value that is
// not an integer or number or is out of range, a repeated key, a repeated model name or no
// `progs` is refused before anything runs, with exit status 2 and no trace.
fn refusals() -> Vec<Case> {
    let long_map = format!("map {}\n", "a".repeat(64));
    let refused: [(&str, &str, &'static str); 48] = [
        ("unknown-key", "bogus 1\n", "unknown key"),
        ("missing-argument", "frames\n", "frames takes"),
        ("extra-argument", "frames 1 2\n", "frames takes"),
        ("not-an-integer", "frames 1.5\n", "frames takes"),
        ("frames-too-many", "frames 1000001\n", "frames takes"),
        ("negative-frames", "frames -1\n", "frames takes"),
        ("repeated-key", "frames 1\nframes 2\n", "may appear once"),
        ("map-name", "map Start\n", "map takes"),
        ("map-name-too-long", &long_map, "map takes"),
        ("maxclients-zero", "maxclients 0\n", "maxclients takes"),
        ("maxclients-17", "maxclients 17\n", "maxclients takes"),
        ("clients-17", "clients 17\n", "clients takes"),
        (
            "clients-above-maxclients",
            "maxclients 2\nclients 3\n",
            "more than maxclients",
        ),
        ("submodels-255", "submodels 255\n", "submodels takes"),
        ("model-five-numbers", "model m 0 0 0 0 0\n", "model takes"),
        ("model-not-a-number", "model m 0 0 0 0 0 x\n", "model takes"),
        ("model-infinite", "model m 0 0 0 0 0 1e39\n", "model takes"),
        (
            "model-repeated",
            "model m 0 0 0 0 0 0\nmodel m 1 1 1 1 1 1\n",
            "repeated",
        ),
        ("cvar-one-argument", "cvar skill\n", "cvar takes"),
        ("frametime-zero", "frametime 0\n", "greater than 0"),
        ("frametime-above-one", "frametime 1.5\n", "greater than 0"),
        ("frametime-text", "frametime fast\n", "frametime takes"),
        ("seed-negative", "seed -1\n", "seed takes"),
        ("progs-extra", "", "progs takes"),
        ("entities-no-path", "entities\n", "entities takes"),
        (
            "input-short",
            "clients 1\ninput 1 1 impulse\n",
            "a field and values",
        ),
        (
            "input-frame-zero",
            "clients 1\ninput 0 1 impulse 1\n",
            "a field and values",
        ),
        (
            "input-after-last-frame",
            "clients 1\nframes 2\ninput 3 1 impulse 1\n",
            "after the last frame",
        ),
        (
            "input-edict-zero",
            "clients 1\ninput 1 0 impulse 1\n",
            "an edict from 1",
        ),
        (
            "input-edict-no-client",
            "clients 1\ninput 1 2 impulse 1\n",
            "not a client that enters",
        ),
        (
            "input-field",
            "clients 1\ninput 1 1 health 1\n",
            "input field is",
        ),
        (
            "input-button-two",
            "clients 1\ninput 1 1 button0 2\n",
            "a button takes",
        ),
        (
            "input-impulse-zero",
            "clients 1\ninput 1 1 impulse 0\n",
            "impulse takes",
        ),
        (
            "input-impulse-256",
            "clients 1\ninput 1 1 impulse 256\n",
            "impulse takes",
        ),
        (
            "input-v-angle-two",
            "clients 1\ninput 1 1 v_angle 1 2\n",
            "v_angle takes",
        ),
        (
            "input-v-angle-text",
            "clients 1\ninput 1 1 v_angle 1 2 x\n",
            "v_angle takes",
        ),
        (
            "too-many-words",
            "frames 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16\n",
            "too many arguments",
        ),
        (
            "entities-two-paths",
            "entities a.ent b.ent\n",
            "entities takes",
        ),
        ("map-two-names", "map a b\n", "map takes"),
        ("maxclients-two", "maxclients 1 2\n", "maxclients takes"),
        ("clients-none", "clients\n", "clients takes"),
        ("submodels-two", "submodels 1 2\n", "submodels takes"),
        ("frametime-two", "frametime 0.1 0.2\n", "frametime takes"),
        ("seed-two", "seed 1 2\n", "seed takes"),
        ("cvar-three", "cvar a b c\n", "cvar takes"),
        (
            "input-button-two-values",
            "clients 1\ninput 1 1 button0 1 0\n",
            "a button takes",
        ),
        (
            "input-impulse-two-values",
            "clients 1\ninput 1 1 impulse 1 2\n",
            "impulse takes",
        ),
        (
            "input-frame-text",
            "clients 1\ninput x 1 impulse 1\n",
            "a field and values",
        ),
    ];
    let mut cases = Vec::new();
    for (name, lines, message) in refused {
        let input = if name == "progs-extra" {
            raw_scenario(b"progs progs.dat other.dat\n", &[])
        } else {
            scenario(lines)
        };
        cases.push(Case::new(name, Program::new(), input).check(Check::Exit(2, message)));
    }
    cases.push(
        Case::new("no-progs", Program::new(), raw_scenario(b"frames 1\n", &[]))
            .check(Check::Exit(2, "no progs")),
    );
    cases.push(
        Case::new(
            "missing-entities",
            Program::new(),
            scenario("entities nowhere.ent\n"),
        )
        .check(Check::Exit(2, "cannot read the entities")),
    );
    cases
}

// spec DIFF-3.1, TRACE-1.3: a progs that is refused, or has fewer than 92 globals or 105
// fields, ends the scenario before the trace header: exit status 2, no trace.
fn refused_progs() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut crc = Program::new();
    crc.crc = 5928;
    cases.push(
        Case::new("progs-crc", crc, scenario(""))
            .check(Check::Exit(2, "progdefs.h is out of date")),
    );
    let mut globals = Program::new();
    globals.truncate_globals(91);
    cases.push(
        Case::new("progs-91-globals", globals, scenario(""))
            .check(Check::Exit(2, "fewer than 92 global slots")),
    );
    let mut fields = Program::new();
    fields.entityfields = 104;
    cases.push(
        Case::new("progs-104-fields", fields, scenario(""))
            .check(Check::Exit(2, "fewer than 105 field slots")),
    );
    let good = Program::new().bytes().unwrap_or_default();
    let mut version = good.clone();
    if let Some(byte) = version.first_mut() {
        *byte = 7;
    }
    let mut case = Case::new("progs-version", Program::new(), scenario(""))
        .check(Check::Exit(2, "wrong version number"));
    case.progs = Some(version);
    cases.push(case);
    let mut short =
        Case::new("progs-short", Program::new(), scenario("")).check(Check::Exit(2, "too short"));
    short.progs = Some(good.get(..30).unwrap_or_default().to_vec());
    cases.push(short);
    // The statements table's count, at byte 12 of the header, made larger than the file.
    let mut outside = good;
    if let Some(count) = outside.get_mut(12..16) {
        count.copy_from_slice(&0x0010_0000_u32.to_le_bytes());
    }
    let mut case = Case::new("progs-table-outside", Program::new(), scenario(""))
        .check(Check::Exit(2, "lies outside the file"));
    case.progs = Some(outside);
    cases.push(case);
    let missing = Case::new(
        "progs-missing",
        Program::new(),
        raw_scenario(b"progs nowhere.dat\n", &[]),
    )
    .check(Check::Exit(2, "cannot open the progs"));
    cases.push(missing);
    cases.extend(progs_header_refusals());
    cases.extend(too_many_field_slots());
    cases
}

// spec EDICT-3.5: a progs with more than 894760 field slots is refused, in scenario mode and in
// vector mode, which does not check the CRC (a finding of the progs fuzz target, whose edict
// size overflowed an int).
fn too_many_field_slots() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, input) in [
        ("progs-too-many-fields", scenario("")),
        (
            "vector-progs-too-many-fields",
            Input::Vector {
                init: "edicts 3\n".to_owned(),
            },
        ),
    ] {
        let mut p = Program::new();
        p.entityfields = 894_761;
        cases.push(
            Case::new(name, p, input).check(Check::Exit(2, "more than 894760 field slots")),
        );
    }
    cases
}

/// The word at byte `offset` of a progs's header.
fn header_word(bytes: &[u8], offset: usize) -> i32 {
    bytes
        .get(offset..offset.saturating_add(4))
        .and_then(|word| <[u8; 4]>::try_from(word).ok())
        .map_or(0, i32::from_le_bytes)
}

// spec DIFF-3.1: a progs whose tables do not lie inside it, whose tables are misaligned, which
// has no string table or a negative field count is refused (PROGS-15), before the trace.
fn progs_header_refusals() -> Vec<Case> {
    let good = Program::new().bytes().unwrap_or_default();
    // Header words: 8 ofs_statements, 12 numstatements, 16 ofs_globaldefs, 20 numglobaldefs,
    // 24 ofs_fielddefs, 28 numfielddefs, 32 ofs_functions, 36 numfunctions, 40 ofs_strings,
    // 44 numstrings, 48 ofs_globals, 52 numglobals, 56 entityfields.
    let shifted = |offset: usize, by: i32| header_word(&good, offset).saturating_add(by);
    let patches: [(&str, usize, i32); 13] = [
        ("progs-statements-before-the-file", 8, -4),
        ("progs-globaldefs-outside", 20, 0x0010_0000),
        ("progs-fielddefs-outside", 28, 0x0010_0000),
        ("progs-functions-outside", 36, 0x0010_0000),
        ("progs-strings-outside", 44, 0x0100_0000),
        ("progs-globals-outside", 52, 0x0010_0000),
        ("progs-no-strings", 44, 0),
        ("progs-statements-misaligned", 8, shifted(8, -1)),
        ("progs-globaldefs-misaligned", 16, shifted(16, -2)),
        ("progs-fielddefs-misaligned", 24, shifted(24, -2)),
        ("progs-functions-misaligned", 32, shifted(32, -2)),
        ("progs-globals-misaligned", 48, shifted(48, -2)),
        ("progs-negative-entityfields", 56, -1),
    ];
    let mut cases = Vec::new();
    for (name, offset, value) in patches {
        let mut bytes = good.clone();
        if let Some(word) = bytes.get_mut(offset..offset.saturating_add(4)) {
            word.copy_from_slice(&value.to_le_bytes());
        }
        let mut case = Case::new(name, Program::new(), scenario(""))
            .check(Check::Exit(2, "lies outside the file"));
        case.progs = Some(bytes);
        cases.push(case);
    }
    cases
}

// spec DIFF-2.1: the last line need not end with a line feed.
fn last_line_without_a_line_feed() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    Case::new(
        "last-line-without-a-line-feed",
        p,
        raw_scenario(b"progs progs.dat\nmap e1_m1\nframes 1", &[]),
    )
    .check(Check::Count(Want::Run(start), 3))
    .check(Check::FinalGlobal(global::MAPNAME, Val::Text("e1_m1")))
}

// spec DIFF-2.1: a scenario named without a directory reads its paths from the working
// directory, which is then its directory.
fn relative_scenario_path() -> Case {
    let mut p = Program::new();
    let start = start_frame(&mut p, Vec::new());
    Case::new(
        "relative-scenario-path",
        p,
        Input::Relative(b"progs progs.dat\nframes 1\n".to_vec()),
    )
    .check(Check::Count(Want::Run(start), 3))
}
