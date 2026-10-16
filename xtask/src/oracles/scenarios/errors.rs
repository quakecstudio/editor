// SPDX-License-Identifier: GPL-3.0-or-later

//! Scenario tests of errors and the end of a scenario (`differential.md` DIFF-8), of what
//! the oracle does where id's code is undefined (DIFF-10.1), and of the oracle's exit statuses
//! (`oracles/README.md`: 0 traced, 2 refused, 3 cannot write, 4 crashed).

use super::cases::{done, f, first_edict, named, spawn, start_frame, worldspawn};
use super::{
    Arg, Case, Check, Input, Program, TracePath, Val, Want, field, global, scenario,
    scenario_with_map, ty,
};

pub(super) fn all() -> Vec<Case> {
    let mut cases = vec![
        runtime_error_in_a_run(),
        null_function_from_the_host(),
        no_free_edicts(),
        not_an_entity(),
        closing_brace_without_data(),
        unknown_spawn_function(),
        unknown_field_name(),
        entity_and_field_keys(),
        error_text_is_joined(),
        objerror_frees_self(),
        break_is_fatal(),
        shutdown_after_an_error(),
        error_while_loading(),
        depth_kept_after_a_host_error(),
        depth_reset_after_a_runtime_error(),
        error_during_shutdown(),
        crash(),
        large_trace(),
        unwritable_trace(),
        unwritable_vector_trace(),
        bad_command_line(),
        console_copied(),
    ];
    cases.extend(vector_refusals());
    if cfg!(target_os = "linux") {
        cases.push(full_device(
            "trace-to-a-full-device",
            scenario("frames 1\n"),
        ));
        cases.push(full_device(
            "vector-trace-to-a-full-device",
            Input::Vector {
                init: "edicts 3\n".to_owned(),
            },
        ));
    }
    cases
}

/// `name` calling `error(text)`.
fn errors(p: &mut Program, name: &str, text: &str) -> i32 {
    let error = p.builtin("error", 10);
    let message = p.string(text);
    let code = p.call(error, &[Arg::F(message)]);
    p.func(name, &done(code))
}

// spec DIFF-8.1, TRACE-4.6, TRACE-4.7: a runtime error writes an error record, in place of the
// step record, with the function and statement that raised it.
fn runtime_error_in_a_run() -> Case {
    let mut p = Program::new();
    let null = p.int(0);
    let statement = p.next_statement();
    let start = start_frame(&mut p, vec![format!("CALL0 {null}")]);
    Case::new("runtime-error", p, scenario("frames 1\n"))
        .check(Check::Last(Want::Error(start, statement, "NULL function")))
        .check(Check::Never(Want::Step(statement)))
}

// spec DIFF-8.1, DIFF-8.2, TRACE-4.1, TRACE-4.7: the host's call of a function that is not one
// writes the run record with the index as passed, then the host error
// `PR_ExecuteProgram: NULL function` with that function and statement -1.
fn null_function_from_the_host() -> Case {
    let mut p = Program::new();
    p.system(global::START_FRAME, 9999);
    Case::new("null-function", p, scenario("frames 1\n"))
        .check(Check::Seq(vec![
            Want::Run(9999),
            Want::Error(9999, -1, "PR_ExecuteProgram: NULL function"),
        ]))
        .check(Check::Last(Want::Error(
            9999,
            -1,
            "PR_ExecuteProgram: NULL function",
        )))
}

// spec DIFF-8.1: an error the host raises while spawning the map, outside any run, has
// function 0 and statement -1: here `ED_Alloc: no free edicts` after 598 map entities.
fn no_free_edicts() -> Case {
    let mut p = Program::new();
    let thing = p.func("thing", &done(Vec::new()));
    let mut map = String::from("{\n\"classname\" \"worldspawn\"\n}\n");
    for _ in 0..600 {
        map.push_str("{\n\"classname\" \"thing\"\n}\n");
    }
    Case::new("no-free-edicts", p, scenario_with_map("", &map))
        .check(Check::Count(Want::Run(thing), 598))
        .check(Check::Last(Want::Error(0, -1, "ED_Alloc: no free edicts")))
}

// spec DIFF-8.1: a map whose text is not entities is the fatal error of EDICT-8.8.
fn not_an_entity() -> Case {
    Case::new(
        "not-an-entity",
        Program::new(),
        scenario_with_map("", "x\n"),
    )
    .check(Check::Last(Want::Error(
        0,
        -1,
        "ED_LoadFromFile: found x when expecting {",
    )))
}

// spec DIFF-8.1: so is an entity that ends before its first key.
fn closing_brace_without_data() -> Case {
    Case::new(
        "closing-brace-without-data",
        Program::new(),
        scenario_with_map("", "{\n\"classname\"\n}\n"),
    )
    .check(Check::Last(Want::Error(
        0,
        -1,
        "ED_ParseEntity: closing brace without data",
    )))
}

// spec DIFF-8.1, DIFF-8.3: a function key naming no function is the host error
// `ED_ParseEdict: parse error`; the map is loading, so nothing shuts down.
fn unknown_spawn_function() -> Case {
    let mut p = Program::new();
    p.field_def("think", ty::FUNCTION, field::THINK);
    let disconnect = named(&mut p, global::CLIENT_DISCONNECT, "disconnect");
    let map = "{\n\"classname\" \"worldspawn\"\n\"think\" \"nowhere\"\n}\n";
    Case::new(
        "unknown-function-key",
        p,
        scenario_with_map("clients 1\n", map),
    )
    .check(Check::Last(Want::Error(
        0,
        -1,
        "ED_ParseEdict: parse error",
    )))
    .check(Check::Never(Want::Run(disconnect)))
}

// spec DIFF-8.1: so is a field key naming no field.
fn unknown_field_name() -> Case {
    let mut p = Program::new();
    p.field_def("pointing", ty::FIELD, 105);
    let map = "{\n\"classname\" \"worldspawn\"\n\"pointing\" \"nowhere\"\n}\n";
    Case::new("unknown-field-key", p, scenario_with_map("", map)).check(Check::Last(Want::Error(
        0,
        -1,
        "ED_ParseEdict: parse error",
    )))
}

// spec DIFF-3.5: entity, field and function keys hold an edict, the global whose slot is the
// named field's offset, and a function index (EDICT-8.2).
fn entity_and_field_keys() -> Case {
    let mut p = Program::new();
    p.set(field::HEALTH, 0x1234);
    p.field_def("owner", ty::ENTITY, 95);
    p.field_def("pointing", ty::FIELD, 105);
    p.field_def("think", ty::FUNCTION, field::THINK);
    p.field_def("health", ty::FLOAT, field::HEALTH);
    let map = "{\n\"classname\" \"worldspawn\"\n\"owner\" \"1\"\n\"pointing\" \"health\"\n\
               \"think\" \"noop\"\n}\n";
    Case::new(
        "entity-and-field-keys",
        p,
        scenario_with_map("frames 0\n", map),
    )
    .check(Check::FinalField(0, 95, Val::Entity(1)))
    .check(Check::FinalField(0, 105, Val::Bits(0x1234)))
    .check(Check::FinalField(0, field::THINK, Val::Bits(1)))
}

// spec DIFF-8.2: the message of `error` is the joined text of its arguments, not id's
// `Program error`.
fn error_text_is_joined() -> Case {
    let mut p = Program::new();
    let error = p.builtin("error", 10);
    let a = p.string("one ");
    let b = p.string("two");
    let code = p.call(error, &[Arg::F(a), Arg::F(b)]);
    let start = start_frame(&mut p, code);
    Case::new("error-text", p, scenario("frames 1\n"))
        .check(Check::Last(Want::ErrorMessage("one two")))
        .check(Check::Count(Want::Run(start), 1))
}

// spec DIFF-8.2: `objerror` carries its joined text too, after freeing `self`.
fn objerror_frees_self() -> Case {
    let mut p = Program::new();
    let objerror = p.builtin("objerror", 11);
    let text = p.string("bad object");
    let e = p.temp();
    let mut code = spawn(&mut p, e);
    code.push(format!("STORE_ENT {e} {}", global::SELF));
    code.extend(p.call(objerror, &[Arg::F(text)]));
    worldspawn(&mut p, code);
    Case::new("objerror", p, scenario("")).check(Check::Last(Want::All(vec![
        Want::ErrorMessage("bad object"),
        Want::Field(2, field::NEXTTHINK, f(-1.0)),
    ])))
}

// spec DIFF-8.2, DIFF-8.6: `break` is the fatal error `break statement` (BUILTIN-16.3): its
// record ends the scenario, with no shutdown.
fn break_is_fatal() -> Case {
    let mut p = Program::new();
    let disconnect = named(&mut p, global::CLIENT_DISCONNECT, "disconnect");
    let brk = p.builtin("break", 6);
    let code = p.call(brk, &[]);
    p.system(global::PLAYER_PRE_THINK, p.next_function());
    p.func("pre", &done(code));
    Case::new("break", p, scenario("clients 1\nframes 2\n"))
        .check(Check::Last(Want::ErrorMessage("break statement")))
        .check(Check::Never(Want::Run(disconnect)))
}

// spec DIFF-8.3: after a runtime or host error with the server active, every client that
// entered gets `ClientDisconnect`, in slot order, with `self` its edict; then the scenario
// ends.
fn shutdown_after_an_error() -> Case {
    let mut p = Program::new();
    let disconnect = named(&mut p, global::CLIENT_DISCONNECT, "disconnect");
    // In the first client's PlayerPreThink, once the clients have entered.
    let boom = errors(&mut p, "boom", "boom");
    p.system(global::PLAYER_PRE_THINK, boom);
    Case::new(
        "shutdown",
        p,
        scenario("maxclients 3\nclients 2\nframes 5\n"),
    )
    .check(Check::Seq(vec![
        Want::ErrorMessage("boom"),
        // `self` is already the first client's edict, whose PlayerPreThink failed.
        Want::Run(disconnect),
        Want::All(vec![
            Want::Run(disconnect),
            Want::Global(global::SELF, Val::Entity(2)),
        ]),
        Want::Exit,
    ]))
    .check(Check::Count(Want::Run(disconnect), 2))
    .check(Check::Count(Want::ErrorMessage("boom"), 1))
    .check(Check::Last(Want::Exit))
}

// spec DIFF-8.3: while the map is loading the server is not active: an error ends the scenario
// after its record.
fn error_while_loading() -> Case {
    let mut p = Program::new();
    let disconnect = named(&mut p, global::CLIENT_DISCONNECT, "disconnect");
    let boom = errors(&mut p, "boom", "early");
    let code = vec![format!("CALL0 {}", p.fref(boom))];
    worldspawn(&mut p, code);
    Case::new("error-while-loading", p, scenario("clients 1\n"))
        .check(Check::Last(Want::ErrorMessage("early")))
        .check(Check::Never(Want::Run(disconnect)))
}

/// `PlayerPreThink` calls `first`, which calls the next, 29 deep, the last running `last` code;
/// `ClientDisconnect` calls a QC function. Returns the disconnect function.
fn deep_calls(p: &mut Program, last: Vec<String>) -> i32 {
    let mut callee = p.func("deep_29", &done(last));
    for depth in (1..29).rev() {
        let target = p.fref(callee);
        callee = p.func(
            &format!("deep_{depth}"),
            &done(vec![format!("CALL0 {target}")]),
        );
    }
    let target = p.fref(callee);
    let pre = p.func("pre", &done(vec![format!("CALL0 {target}")]));
    p.system(global::PLAYER_PRE_THINK, pre);
    let inner = p.func("inner", &done(Vec::new()));
    let inner = p.fref(inner);
    let disconnect = p.func("disconnect", &done(vec![format!("CALL0 {inner}")]));
    p.system(global::CLIENT_DISCONNECT, disconnect);
    disconnect
}

// spec DIFF-8.4, DIFF-8.5: a host error keeps the call depth, so after `error` 30 calls deep
// the first QC call of `ClientDisconnect` is a stack overflow, which ends the scenario.
fn depth_kept_after_a_host_error() -> Case {
    let mut p = Program::new();
    let error = p.builtin("error", 10);
    let text = p.string("deep");
    let last = p.call(error, &[Arg::F(text)]);
    let disconnect = deep_calls(&mut p, last);
    Case::new("depth-kept", p, scenario("clients 1\nframes 1\n"))
        .check(Check::Seq(vec![
            Want::ErrorMessage("deep"),
            Want::Run(disconnect),
            Want::ErrorMessage("stack overflow"),
        ]))
        .check(Check::Last(Want::ErrorMessage("stack overflow")))
}

// spec DIFF-8.4: a runtime error sets the depth to 0, so the same calls after one run.
fn depth_reset_after_a_runtime_error() -> Case {
    let mut p = Program::new();
    let null = p.int(0);
    let disconnect = deep_calls(&mut p, vec![format!("CALL0 {null}")]);
    Case::new("depth-reset", p, scenario("clients 1\nframes 1\n"))
        .check(Check::Seq(vec![
            Want::ErrorMessage("NULL function"),
            Want::Run(disconnect),
            Want::Exit,
        ]))
        .check(Check::Never(Want::ErrorMessage("stack overflow")))
        .check(Check::Last(Want::Exit))
}

// spec DIFF-8.5: an error during the shutdown writes its record and ends the scenario: the
// later slots' `ClientDisconnect` does not run.
fn error_during_shutdown() -> Case {
    let mut p = Program::new();
    let boom = errors(&mut p, "boom", "first");
    p.system(global::PLAYER_PRE_THINK, boom);
    let disconnect = errors(&mut p, "disconnect", "second");
    p.system(global::CLIENT_DISCONNECT, disconnect);
    Case::new(
        "error-during-shutdown",
        p,
        scenario("maxclients 2\nclients 2\n"),
    )
    .check(Check::Seq(vec![
        Want::ErrorMessage("first"),
        Want::Run(disconnect),
        Want::ErrorMessage("second"),
    ]))
    .check(Check::Count(Want::Run(disconnect), 1))
    .check(Check::Last(Want::ErrorMessage("second")))
}

// spec DIFF-10.1: where id's VM is undefined the oracle may crash: a load through an entity
// value two gigabytes past the edicts is a crash, exit status 4, after the records so far.
fn crash() -> Case {
    let mut p = Program::new();
    let far = p.int(0x7fff_fff0);
    let field = p.int(0x3fff_ffff);
    let out = p.temp();
    let marker = p.temp();
    let one = p.float(1.0);
    let crash = p.func(
        "crash",
        &done(vec![
            format!("STORE_F {one} {marker}"),
            format!("LOAD_F {far} {field} {out}"),
        ]),
    );
    Case::new(
        "crash",
        p,
        Input::Vector {
            init: format!("edicts 3\nentry {crash}\n"),
        },
    )
    .check(Check::Exit(4, "crashed"))
    .check(Check::Seq(vec![
        Want::Run(crash),
        Want::Global(marker, f(1.0)),
    ]))
}

// spec TRACE-5.6: a trace of more than a megabyte, which the oracle writes in parts as it goes,
// decodes whole: ten frames of a loop of 20000 statements.
fn large_trace() -> Case {
    let mut p = Program::new();
    let counter = p.temp();
    let zero = p.float(0.0);
    let one = p.float(1.0);
    let limit = p.float(5000.0);
    let below = p.temp();
    let start = start_frame(
        &mut p,
        vec![
            format!("STORE_F {zero} {counter}"),
            format!("ADD_F {counter} {one} {counter}"),
            format!("LT {counter} {limit} {below}"),
            format!("IF {below} -2"),
        ],
    );
    Case::new("large-trace", p, scenario("frames 10\n"))
        .check(Check::Count(Want::Run(start), 12))
        .check(Check::Count(Want::Global(counter, f(5000.0)), 12))
        .check(Check::Last(Want::Exit))
}

/// Vector mode's refusals that the probes of `probes.rs` cannot write as constant files: the
/// exit status is 2, with no trace.
fn vector_refusals() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, init, message) in [
        (
            "vector-long-line",
            format!("edicts 3\n{}\n", "x".repeat(600)),
            "line too long",
        ),
        (
            "vector-bad-field-value",
            "edicts 3\nfield 1 105 bits zz\n".to_owned(),
            "bad value",
        ),
        (
            "vector-unknown-line",
            "edicts 3\nbogus\n".to_owned(),
            "unknown line",
        ),
        (
            "vector-entity-past-int",
            "edicts 3\nglobal 100 entity 4000000\n".to_owned(),
            "bad value",
        ),
        (
            "vector-pointer-past-int",
            "edicts 3\nglobal 100 pointer 4000000 1\n".to_owned(),
            "bad value",
        ),
    ] {
        cases.push(
            Case::new(name, Program::new(), Input::Vector { init }).check(Check::Exit(2, message)),
        );
    }
    let mut bad_progs = Case::new(
        "vector-progs-refused",
        Program::new(),
        Input::Vector {
            init: "edicts 3\n".to_owned(),
        },
    )
    .check(Check::Exit(2, "too short"));
    bad_progs.progs = Some(vec![0; 8]);
    cases.push(bad_progs);
    cases.push(
        Case::new(
            "vector-init-missing",
            Program::new(),
            Input::Args(vec![
                "vector".to_owned(),
                "vector-init-missing/progs.dat".to_owned(),
                "vector-init-missing/nowhere.txt".to_owned(),
                "vector-init-missing/trace.bin".to_owned(),
            ]),
        )
        .check(Check::Exit(2, "cannot open")),
    );
    // The init file's values are set before the header, so the program copies one to show it;
    // the file's last line has no line feed.
    let mut p = Program::new();
    let from = p.temp();
    let to = p.temp();
    let copy = p.func("copy", &done(vec![format!("STORE_F {from} {to}")]));
    cases.push(
        Case::new(
            "vector-upper-case-hex",
            p,
            Input::Vector {
                init: format!("edicts 3\nglobal {from} bits ABCDEF\nentry {copy}"),
            },
        )
        .check(Check::FinalGlobal(to, Val::Bits(0x00ab_cdef))),
    );
    cases
}

/// A run whose trace goes to Linux's `/dev/full`, which opens but takes no byte.
fn full_device(name: &str, input: Input) -> Case {
    let mut case = Case::new(name, Program::new(), input).check(Check::Exit(3, "cannot write"));
    case.trace = TracePath::Device("/dev/full");
    case
}

// spec DIFF-10.1: a trace that cannot be written is exit status 3 in scenario mode.
fn unwritable_trace() -> Case {
    let mut case = Case::new("unwritable-trace", Program::new(), scenario("frames 1\n"))
        .check(Check::Exit(3, "cannot write"));
    case.trace = TracePath::Missing;
    case
}

// spec DIFF-10.1: and in vector mode.
fn unwritable_vector_trace() -> Case {
    let mut case = Case::new(
        "unwritable-vector-trace",
        Program::new(),
        Input::Vector {
            init: "edicts 3\n".to_owned(),
        },
    )
    .check(Check::Exit(3, "cannot write"));
    case.trace = TracePath::Missing;
    case
}

// spec DIFF-2.9: a command line the oracle does not take is refused with exit status 2.
fn bad_command_line() -> Case {
    Case::new(
        "bad-command-line",
        Program::new(),
        Input::Args(vec!["scenario".to_owned()]),
    )
    .check(Check::Exit(2, "usage: vm-oracle"))
}

// spec DIFF-6.5: the console is not compared; `--console` copies it to standard error.
fn console_copied() -> Case {
    let mut p = Program::new();
    let e = p.temp();
    let error = p.builtin("error", 10);
    let text = p.string("shown");
    let mut code = first_edict(&mut p, e);
    code.extend(p.call(error, &[Arg::F(text)]));
    worldspawn(&mut p, code);
    let mut case = Case::new("console-copied", p, scenario(""))
        .check(Check::Exit(0, "SERVER ERROR"))
        .check(Check::Last(Want::ErrorMessage("shown")));
    case.console = true;
    case
}
