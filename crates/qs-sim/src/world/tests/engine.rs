// SPDX-License-Identifier: GPL-3.0-or-later

//! The builtins and settings that read the engine's own state: `checkclient`'s visibility,
//! `aim`, the server settings, the client's signon stages and a plane's sign.

use qc_progs::Progs;

use super::{Running, entity, player_progs, run_on, running_from};
use crate::fixtures::{CONTENTS_EMPTY, CONTENTS_SOLID, MapSpec, plane};
use crate::{LoadOptions, Signon, UserCmd};
use qs_bsp::{ClipNode, Node};

/// Two open leaves either side of x = 0: leaf 1 behind (x <= 0), leaf 2 in front; `rows` is
/// what each sees, leaf 1's then leaf 2's.
fn two_rooms(rows: [u8; 2]) -> MapSpec {
    MapSpec {
        planes: vec![plane([1.0, 0.0, 0.0], 0.0, 0)],
        nodes: vec![Node {
            planenum: 0,
            children: [-3, -2],
            mins: [0.0; 3],
            maxs: [0.0; 3],
            firstface: 0,
            numfaces: 0,
        }],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY, CONTENTS_EMPTY],
        clipnodes: vec![ClipNode {
            planenum: 0,
            children: [CONTENTS_EMPTY, CONTENTS_EMPTY],
        }],
        visibility: rows.to_vec(),
        visofs: vec![-1, 0, 1],
    }
}

/// The player at x = -100, leaf 1, and a looker at x = 100, leaf 2, whose `enemy` is what
/// `checkclient` gives it after two frames.
fn looked(rows: [u8; 2]) -> Running {
    let more = "entity() checkclient = #17;\n\
                void() look = { self.enemy = checkclient(); self.nextthink = time + 0.05; };\n\
                void() looker = { self.nextthink = time + 0.05; self.think = look; };\n";
    let put = "self.movetype = 0; setorigin(self, '-100 0 0');";
    let progs = player_progs(put, "", more);
    let mut run = running_from(
        &two_rooms(rows),
        progs,
        &entity("looker", "100 0 0", ""),
        Vec::new(),
        LoadOptions::default(),
    );
    run.connect();
    run.frames(2, 0.1).unwrap();
    run
}

// spec WORLD-8.7: checkclient gives the player only when the caller's eye leaf is in the
// player's PVS
#[test]
fn checkclient_tests_the_players_pvs() {
    let hidden = looked([0x01, 0x02]);
    assert_eq!(
        hidden.world.entity_field(2, "enemy"),
        0,
        "leaf 2 is not in leaf 1's PVS"
    );
    let seen = looked([0x03, 0x03]);
    assert_eq!(
        seen.world.entity_field(2, "enemy"),
        seen.world.entity_value(1)
    );
}

const AIMER: &str = "vector(entity e, float speed) aim = #44;\n\
    void(vector v) makevectors = #1;\n\
    void(string var, string val) cvar_set = #72;\n\
    void() dummy = { self.takedamage = 2; self.solid = 2; \
    setsize(self, '-16 -16 -24', '16 16 32'); setorigin(self, self.origin); };\n\
    void() shooter = { makevectors('0 0 0'); self.movedir = aim(self, 1000); };\n\
    void() narrow = { cvar_set(\"sv_aim\", \"1\"); };\n";

// spec WORLD-8.8: aim turns toward a target to damage within sv_aim's cone that a line
// reaches, and is v_forward when none is
#[test]
fn aim_turns_toward_a_target_in_the_cone() {
    let entities = [
        entity("dummy", "200 0 50", ""),
        entity("shooter", "0 0 0", ""),
    ]
    .concat();
    let run = run_on(&MapSpec::default(), AIMER, &entities);
    let [x, y, z] = run.world.vector(3, "movedir");
    // toward (200, 0, 50): (200, 0, 50) normalized.
    assert!((x - 0.970_142_5).abs() < 1e-5 && y == 0.0 && (z - 0.242_535_6).abs() < 1e-5);
    let narrow = [
        entity("narrow", "0 0 0", ""),
        entity("dummy", "200 0 50", ""),
        entity("shooter", "0 0 0", ""),
    ]
    .concat();
    let run = run_on(&MapSpec::default(), AIMER, &narrow);
    assert_eq!(
        run.world.vector(4, "movedir"),
        [1.0, 0.0, 0.0],
        "sv_aim 1: no cone"
    );
}

// spec PHYS-12.1: the server's settings exist with id's defaults
// spec PHYS-12.2: the physics read them, so a progs that sets one changes the game
#[test]
fn the_settings_are_read() {
    let source = "float(string s) cvar = #45;\n\
        void(string var, string val) cvar_set = #72;\n\
        void() reader = { self.frags = cvar(\"sv_maxspeed\"); self.health = cvar(\"noexit\"); \
        self.armorvalue = cvar(\"sv_aim\"); };\n\
        void() light = { cvar_set(\"sv_gravity\", \"100\"); };\n\
        void() thrown = { self.movetype = 6; self.solid = 2; \
        setsize(self, '-1 -1 -1', '1 1 1'); setorigin(self, self.origin); };\n";
    let entities = [
        entity("reader", "0 0 0", ""),
        entity("light", "0 0 0", ""),
        entity("thrown", "50 0 1000", ""),
    ]
    .concat();
    let mut run = run_on(&MapSpec::default(), source, &entities);
    assert_eq!(run.world.float(2, "frags"), 320.0);
    assert_eq!(run.world.float(2, "health"), 0.0);
    assert_eq!(run.world.float(2, "armorvalue"), 0.93);
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.vector(4, "velocity")[2], -10.0, "sv_gravity 100");
}

// spec PHYS-6.3: sv_maxvelocity is compared as id does, so a negative one bounds without
// failing
#[test]
fn a_negative_max_velocity_does_not_fail() {
    let source = "void(string var, string val) cvar_set = #72;\n\
        void() odd = { cvar_set(\"sv_maxvelocity\", \"-5\"); };\n\
        void() thrown = { self.movetype = 6; self.solid = 2; \
        setsize(self, '-1 -1 -1', '1 1 1'); setorigin(self, self.origin); };\n";
    let entities = [
        entity("odd", "0 0 0", ""),
        entity("thrown", "50 0 1000", ""),
    ]
    .concat();
    let mut run = run_on(&MapSpec::default(), source, &entities);
    run.frames(1, 0.1).unwrap();
    // `if (v > max) v = max`: 0 > -5, so x is -5.
    assert_eq!(run.world.vector(3, "velocity")[0], -5.0);
}

// spec PHYS-10.7: the signon's stages run one at a time: the parms at connect, the spawn
// functions at spawn, the moves only after begin
#[test]
fn the_signon_runs_a_stage_at_a_time() {
    let more = "void() SetNewParms = { parm1 = 42; };\n";
    let put = "self.frags = parm1; self.movetype = 0; setorigin(self, '0 0 100');";
    let progs = player_progs(put, "", more);
    let bytes = progs.clone();
    let mut run = running_from(
        &MapSpec::default(),
        progs,
        "",
        Vec::new(),
        LoadOptions::default(),
    );
    let read = Progs::read(&bytes).unwrap();
    run.world.signon(&read, Signon::Connect).unwrap();
    assert_eq!(run.world.float(1, "frags"), 0.0, "not spawned");
    run.world.signon(&read, Signon::Prespawn).unwrap();
    run.world.signon(&read, Signon::Spawn).unwrap();
    assert_eq!(run.world.float(1, "frags"), 42.0, "SetNewParms' parm1");
    let turn = UserCmd {
        angles: [0.0, 90.0, 0.0],
        ..UserCmd::default()
    };
    run.world.set_command(turn);
    run.frames(1, 0.1).unwrap();
    assert_eq!(
        run.world.vector(1, "v_angle")[1],
        0.0,
        "no moves before begin"
    );
    run.world.signon(&read, Signon::Begin).unwrap();
    run.world.set_command(turn);
    run.frames(1, 0.1).unwrap();
    assert_eq!(run.world.vector(1, "v_angle")[1], 90.0);
}

// spec WORLD-3.7: a plane hit from behind has its normal negated as 0 - n, so its zero
// components are +0
#[test]
fn a_plane_hit_from_behind_has_positive_zeros() {
    // The plane faces -x; its front, x < 0, is solid, and the trace comes from x > 0. Its
    // type is 3, not axial: an axial type reads `p[type] - dist` as if the normal were +x.
    let spec = MapSpec {
        planes: vec![plane([-1.0, 0.0, 0.0], 0.0, 3)],
        nodes: vec![Node {
            planenum: 0,
            children: [-1, -2],
            mins: [0.0; 3],
            maxs: [0.0; 3],
            firstface: 0,
            numfaces: 0,
        }],
        leafs: vec![CONTENTS_SOLID, CONTENTS_EMPTY],
        ..MapSpec::default()
    };
    let source = "void() tracer = { traceline('10 0 5', '-10 0 5', 1, self); \
                  self.movedir = trace_plane_normal; self.frags = trace_fraction; };\n";
    let run = run_on(&spec, source, &entity("tracer", "0 0 0", ""));
    let [x, y, z] = run.world.vector(2, "movedir");
    assert!(run.world.float(2, "frags") < 1.0, "the plane is hit");
    assert_eq!(x, 1.0);
    assert_eq!((y.to_bits(), z.to_bits()), (0, 0), "+0, not -0");
}
