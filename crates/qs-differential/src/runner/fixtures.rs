// SPDX-License-Identifier: GPL-3.0-or-later

//! The differential scenarios for each corpus codebase (`docs/spec/differential.md` DIFF-2).
//!
//! A scenario names a map's entities and the settings of one run (DIFF-2). To exercise as much
//! of a codebase as the mock engine allows (DIFF-1.5: nothing moves and nothing collides,
//! DIFF-5.8), the scenarios spawn every entity class the codebase defines, let a player enter
//! and cycle its weapons, impulses and view angles, and run many server frames so monsters and
//! other entities think (DIFF-5.6). The entity lumps are hand-authored here rather than taken
//! from a fetched map so the fast CI subset needs no map download.
//!
//! These are the classnames and spawn keys id's QuakeC 1.01 and the rerelease define, in the
//! BSP entity-lump syntax EDICT-8 reads (`{ "key" "value" ... }`). A classname with no spawn
//! function in a codebase is freed, not spawned (EDICT-8.5), so one shared superset is safe
//! across codebases: each spawns only the classes it knows.

/// One scenario to run: a name, the directives after the `progs` line (DIFF-2.2), and the
/// entity lump the scenario's `entities` line names, or `None` for the one `worldspawn`
/// default (DIFF-2.2).
#[derive(Clone, Debug)]
pub struct ScenarioSpec {
    /// A short name, used for the scratch directory and the report.
    pub name: &'static str,
    /// The scenario directives after `progs progs.dat` (DIFF-2.2).
    pub directives: String,
    /// The entity lump, or `None` for the default world.
    pub entities: Option<Vec<u8>>,
}

/// The scenarios for codebase `id`. The id 1.01 and rerelease codebases share the vanilla
/// entity set; each ignores the classes it has no spawn function for (EDICT-8.5).
#[must_use]
pub fn scenarios(_id: &str) -> Vec<ScenarioSpec> {
    vec![
        ScenarioSpec {
            name: "worldspawn",
            directives: "map start\nclients 1\nframes 100\n".to_owned(),
            entities: Some(entities(MINIMAL)),
        },
        ScenarioSpec {
            name: "monsters-and-items",
            directives: concat!(
                "map e1m1\nmaxclients 4\nclients 1\nsubmodels 4\ncvar skill 1\nframes 300\n",
                "model *1 -32 -32 -32 32 32 32\nmodel *2 -32 -32 -32 32 32 32\n",
                "model *3 -32 -32 -32 32 32 32\nmodel *4 -32 -32 -32 32 32 32\n",
                "input 10 1 impulse 9\ninput 20 1 button0 1\ninput 25 1 button0 0\n",
                "input 30 1 impulse 2\ninput 40 1 button0 1\ninput 45 1 button0 0\n",
                "input 50 1 impulse 3\ninput 60 1 button0 1\ninput 65 1 button0 0\n",
                "input 70 1 impulse 4\ninput 80 1 button0 1\ninput 85 1 button0 0\n",
                "input 90 1 impulse 5\ninput 100 1 button0 1\ninput 105 1 button0 0\n",
                "input 110 1 impulse 6\ninput 120 1 button0 1\ninput 125 1 button0 0\n",
                "input 130 1 impulse 7\ninput 140 1 button0 1\ninput 145 1 button0 0\n",
                "input 150 1 impulse 8\ninput 160 1 button0 1\ninput 165 1 button0 0\n",
                "input 170 1 v_angle 0 90 0\ninput 180 1 button2 1\ninput 185 1 button2 0\n",
                "input 190 1 impulse 10\ninput 200 1 button0 1\ninput 205 1 button0 0\n",
                "input 210 1 impulse 11\ninput 220 1 impulse 255\n"
            )
            .to_owned(),
            entities: Some(entities(FULL)),
        },
        ScenarioSpec {
            name: "deathmatch",
            directives: concat!(
                "map dm3\nmaxclients 4\nclients 2\ncvar deathmatch 1\nframes 200\n",
                "input 10 1 impulse 1\ninput 20 1 button0 1\ninput 25 1 button0 0\n",
                "input 30 2 impulse 10\ninput 40 2 button0 1\ninput 45 2 button0 0\n"
            )
            .to_owned(),
            entities: Some(entities(DEATHMATCH)),
        },
        ScenarioSpec {
            name: "coop",
            directives: concat!(
                "map e1m2\nmaxclients 4\nclients 2\ncvar coop 1\ncvar skill 2\nsubmodels 4\n",
                "frames 200\nmodel *1 -32 -32 -32 32 32 32\nmodel *2 -32 -32 -32 32 32 32\n",
                "model *3 -32 -32 -32 32 32 32\nmodel *4 -32 -32 -32 32 32 32\n"
            )
            .to_owned(),
            entities: Some(entities(FULL)),
        },
        // Team play: the rogue mission pack's capture-the-flag (`teamplay` 4, `TEAM_CTF`) and the
        // rerelease's CTF, whose flags spawn only under deathmatch.
        ScenarioSpec {
            name: "ctf",
            directives: concat!(
                "map dm3\nmaxclients 4\nclients 2\ncvar deathmatch 1\ncvar teamplay 4\n",
                "frames 300\n",
                "input 10 1 impulse 1\ninput 20 1 button0 1\ninput 25 1 button0 0\n",
                "input 30 2 impulse 2\ninput 40 2 button0 1\ninput 45 2 button0 0\n"
            )
            .to_owned(),
            entities: Some(entities(CTF)),
        },
    ]
}

/// Joins entity blocks into a BSP entity lump (DIFF-2.5); each block is already
/// `{ "key" "value" ... }`.
fn entities(blocks: &[&str]) -> Vec<u8> {
    blocks.join("\n").into_bytes()
}

/// A worldspawn and a player start: the smallest spawnable map (EDICT-8).
const MINIMAL: &[&str] = &[
    "{\n\"classname\" \"worldspawn\"\n\"message\" \"differential\"\n\"worldtype\" \"0\"\n\"sounds\" \"1\"\n}",
    "{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n\"angle\" \"90\"\n}",
];

/// A full vanilla entity set: the world, starts, every monster, item, weapon, light, and the
/// brush-model entities (doors, plats, buttons, triggers) on submodels `*1` to `*4`.
const FULL: &[&str] = &[
    "{\n\"classname\" \"worldspawn\"\n\"message\" \"differential\"\n\"worldtype\" \"0\"\n\"sounds\" \"1\"\n}",
    "{\n\"classname\" \"info_player_start\"\n\"origin\" \"0 0 24\"\n\"angle\" \"90\"\n}",
    "{\n\"classname\" \"info_player_start2\"\n\"origin\" \"64 0 24\"\n}",
    "{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"128 0 24\"\n}",
    "{\n\"classname\" \"info_player_coop\"\n\"origin\" \"192 0 24\"\n}",
    "{\n\"classname\" \"info_teleport_destination\"\n\"origin\" \"256 0 24\"\n\"targetname\" \"td1\"\n}",
    "{\n\"classname\" \"info_null\"\n\"origin\" \"0 64 0\"\n}",
    "{\n\"classname\" \"info_notnull\"\n\"origin\" \"0 128 0\"\n}",
    "{\n\"classname\" \"info_intermission\"\n\"origin\" \"0 0 256\"\n\"mangle\" \"20 240 0\"\n}",
    "{\n\"classname\" \"light\"\n\"origin\" \"0 0 64\"\n\"light\" \"200\"\n}",
    "{\n\"classname\" \"light_fluoro\"\n\"origin\" \"0 32 64\"\n}",
    "{\n\"classname\" \"light_fluorospark\"\n\"origin\" \"0 64 64\"\n}",
    "{\n\"classname\" \"light_globe\"\n\"origin\" \"0 96 64\"\n}",
    "{\n\"classname\" \"light_torch_small_walltorch\"\n\"origin\" \"32 0 64\"\n}",
    "{\n\"classname\" \"light_flame_large_yellow\"\n\"origin\" \"64 0 64\"\n}",
    "{\n\"classname\" \"light_flame_small_yellow\"\n\"origin\" \"96 0 64\"\n}",
    "{\n\"classname\" \"light_flame_small_white\"\n\"origin\" \"128 0 64\"\n}",
    "{\n\"classname\" \"item_shells\"\n\"origin\" \"32 32 0\"\n}",
    "{\n\"classname\" \"item_spikes\"\n\"origin\" \"64 32 0\"\n}",
    "{\n\"classname\" \"item_rockets\"\n\"origin\" \"96 32 0\"\n}",
    "{\n\"classname\" \"item_cells\"\n\"origin\" \"128 32 0\"\n}",
    "{\n\"classname\" \"item_health\"\n\"origin\" \"160 32 0\"\n}",
    "{\n\"classname\" \"item_health\"\n\"origin\" \"160 64 0\"\n\"spawnflags\" \"1\"\n}",
    "{\n\"classname\" \"item_health\"\n\"origin\" \"160 96 0\"\n\"spawnflags\" \"2\"\n}",
    "{\n\"classname\" \"item_armor1\"\n\"origin\" \"192 32 0\"\n}",
    "{\n\"classname\" \"item_armor2\"\n\"origin\" \"224 32 0\"\n}",
    "{\n\"classname\" \"item_armorInv\"\n\"origin\" \"256 32 0\"\n}",
    "{\n\"classname\" \"item_artifact_super_damage\"\n\"origin\" \"0 160 0\"\n}",
    "{\n\"classname\" \"item_artifact_invulnerability\"\n\"origin\" \"32 160 0\"\n}",
    "{\n\"classname\" \"item_artifact_envirosuit\"\n\"origin\" \"64 160 0\"\n}",
    "{\n\"classname\" \"item_artifact_invisibility\"\n\"origin\" \"96 160 0\"\n}",
    "{\n\"classname\" \"item_key1\"\n\"origin\" \"128 160 0\"\n}",
    "{\n\"classname\" \"item_key2\"\n\"origin\" \"160 160 0\"\n}",
    "{\n\"classname\" \"item_sigil\"\n\"origin\" \"192 160 0\"\n\"spawnflags\" \"1\"\n}",
    "{\n\"classname\" \"weapon_supershotgun\"\n\"origin\" \"0 -32 0\"\n}",
    "{\n\"classname\" \"weapon_nailgun\"\n\"origin\" \"32 -32 0\"\n}",
    "{\n\"classname\" \"weapon_supernailgun\"\n\"origin\" \"64 -32 0\"\n}",
    "{\n\"classname\" \"weapon_grenadelauncher\"\n\"origin\" \"96 -32 0\"\n}",
    "{\n\"classname\" \"weapon_rocketlauncher\"\n\"origin\" \"128 -32 0\"\n}",
    "{\n\"classname\" \"weapon_lightning\"\n\"origin\" \"160 -32 0\"\n}",
    "{\n\"classname\" \"monster_army\"\n\"origin\" \"256 0 24\"\n\"angle\" \"180\"\n}",
    "{\n\"classname\" \"monster_dog\"\n\"origin\" \"256 64 24\"\n}",
    "{\n\"classname\" \"monster_ogre\"\n\"origin\" \"256 128 24\"\n}",
    "{\n\"classname\" \"monster_knight\"\n\"origin\" \"256 192 24\"\n}",
    "{\n\"classname\" \"monster_zombie\"\n\"origin\" \"320 0 24\"\n}",
    "{\n\"classname\" \"monster_wizard\"\n\"origin\" \"320 64 24\"\n}",
    "{\n\"classname\" \"monster_demon1\"\n\"origin\" \"320 128 24\"\n}",
    "{\n\"classname\" \"monster_shambler\"\n\"origin\" \"320 192 24\"\n}",
    "{\n\"classname\" \"monster_boss\"\n\"origin\" \"384 0 24\"\n}",
    "{\n\"classname\" \"monster_enforcer\"\n\"origin\" \"384 64 24\"\n}",
    "{\n\"classname\" \"monster_hell_knight\"\n\"origin\" \"384 128 24\"\n}",
    "{\n\"classname\" \"monster_shalrath\"\n\"origin\" \"384 192 24\"\n}",
    "{\n\"classname\" \"monster_tarbaby\"\n\"origin\" \"448 0 24\"\n}",
    "{\n\"classname\" \"monster_fish\"\n\"origin\" \"448 64 24\"\n}",
    "{\n\"classname\" \"monster_oldone\"\n\"origin\" \"448 128 24\"\n}",
    "{\n\"classname\" \"event_lightning\"\n\"origin\" \"448 192 24\"\n}",
    "{\n\"classname\" \"func_door\"\n\"model\" \"*1\"\n\"angle\" \"-1\"\n\"targetname\" \"door1\"\n}",
    "{\n\"classname\" \"func_door_secret\"\n\"model\" \"*2\"\n}",
    "{\n\"classname\" \"func_plat\"\n\"model\" \"*3\"\n\"height\" \"128\"\n}",
    "{\n\"classname\" \"func_button\"\n\"model\" \"*4\"\n\"target\" \"door1\"\n}",
    "{\n\"classname\" \"trigger_once\"\n\"target\" \"door1\"\n\"origin\" \"0 0 0\"\n}",
    "{\n\"classname\" \"trigger_multiple\"\n\"wait\" \"1\"\n\"origin\" \"16 0 0\"\n}",
    "{\n\"classname\" \"trigger_teleport\"\n\"target\" \"td1\"\n\"origin\" \"32 0 0\"\n}",
    "{\n\"classname\" \"trigger_changelevel\"\n\"map\" \"e1m2\"\n\"origin\" \"48 0 0\"\n}",
    "{\n\"classname\" \"trigger_setskill\"\n\"message\" \"2\"\n\"origin\" \"64 0 0\"\n}",
    "{\n\"classname\" \"trigger_onlyregistered\"\n\"origin\" \"80 0 0\"\n}",
    "{\n\"classname\" \"trigger_secret\"\n\"origin\" \"96 0 0\"\n}",
    "{\n\"classname\" \"trigger_counter\"\n\"count\" \"2\"\n\"origin\" \"112 0 0\"\n}",
    "{\n\"classname\" \"trigger_push\"\n\"angle\" \"0\"\n\"speed\" \"1000\"\n\"origin\" \"128 0 0\"\n}",
    "{\n\"classname\" \"trigger_hurt\"\n\"dmg\" \"5\"\n\"origin\" \"144 0 0\"\n}",
    "{\n\"classname\" \"trigger_monsterjump\"\n\"origin\" \"160 0 0\"\n}",
    "{\n\"classname\" \"trigger_relay\"\n\"target\" \"door1\"\n\"origin\" \"176 0 0\"\n}",
    "{\n\"classname\" \"func_wall\"\n\"origin\" \"192 0 0\"\n}",
    "{\n\"classname\" \"func_illusionary\"\n\"origin\" \"208 0 0\"\n}",
    "{\n\"classname\" \"func_episodegate\"\n\"spawnflags\" \"1\"\n\"origin\" \"224 0 0\"\n}",
    "{\n\"classname\" \"func_bossgate\"\n\"origin\" \"240 0 0\"\n}",
    "{\n\"classname\" \"misc_fireball\"\n\"origin\" \"256 -64 0\"\n}",
    "{\n\"classname\" \"misc_explobox\"\n\"origin\" \"288 -64 0\"\n}",
    "{\n\"classname\" \"misc_explobox2\"\n\"origin\" \"320 -64 0\"\n}",
    "{\n\"classname\" \"misc_noisemaker\"\n\"origin\" \"352 -64 0\"\n}",
    "{\n\"classname\" \"ambient_drip\"\n\"origin\" \"0 0 32\"\n}",
    "{\n\"classname\" \"ambient_drone\"\n\"origin\" \"16 0 32\"\n}",
    "{\n\"classname\" \"ambient_comp_hum\"\n\"origin\" \"32 0 32\"\n}",
    "{\n\"classname\" \"ambient_flouro_buzz\"\n\"origin\" \"48 0 32\"\n}",
    "{\n\"classname\" \"ambient_light_buzz\"\n\"origin\" \"64 0 32\"\n}",
    "{\n\"classname\" \"ambient_suck_wind\"\n\"origin\" \"80 0 32\"\n}",
    "{\n\"classname\" \"ambient_swamp1\"\n\"origin\" \"96 0 32\"\n}",
    "{\n\"classname\" \"ambient_swamp2\"\n\"origin\" \"112 0 32\"\n}",
    "{\n\"classname\" \"ambient_thunder\"\n\"origin\" \"128 0 32\"\n}",
];

/// A deathmatch map: the world, several deathmatch starts and every weapon and ammo, so the
/// deathmatch item and weapon logic runs (`items.qc`, `weapons.qc`).
/// The deathmatch set and both teams' flags.
const CTF: &[&str] = &[
    "{\n\"classname\" \"worldspawn\"\n\"message\" \"ctf\"\n}",
    "{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"0 0 24\"\n}",
    "{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"128 0 24\"\n}",
    "{\n\"classname\" \"info_player_team1\"\n\"origin\" \"0 128 24\"\n}",
    "{\n\"classname\" \"info_player_team2\"\n\"origin\" \"128 128 24\"\n}",
    "{\n\"classname\" \"item_flag_team1\"\n\"origin\" \"-128 0 24\"\n}",
    "{\n\"classname\" \"item_flag_team2\"\n\"origin\" \"256 0 24\"\n}",
    "{\n\"classname\" \"weapon_supershotgun\"\n\"origin\" \"0 -32 0\"\n}",
    "{\n\"classname\" \"weapon_rocketlauncher\"\n\"origin\" \"64 -32 0\"\n}",
    "{\n\"classname\" \"item_rockets\"\n\"origin\" \"0 32 0\"\n}",
];

const DEATHMATCH: &[&str] = &[
    "{\n\"classname\" \"worldspawn\"\n\"message\" \"dm\"\n}",
    "{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"0 0 24\"\n}",
    "{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"128 0 24\"\n}",
    "{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"0 128 24\"\n}",
    "{\n\"classname\" \"info_player_deathmatch\"\n\"origin\" \"128 128 24\"\n}",
    "{\n\"classname\" \"weapon_supershotgun\"\n\"origin\" \"0 -32 0\"\n}",
    "{\n\"classname\" \"weapon_nailgun\"\n\"origin\" \"32 -32 0\"\n}",
    "{\n\"classname\" \"weapon_rocketlauncher\"\n\"origin\" \"64 -32 0\"\n}",
    "{\n\"classname\" \"weapon_lightning\"\n\"origin\" \"96 -32 0\"\n}",
    "{\n\"classname\" \"item_rockets\"\n\"origin\" \"0 32 0\"\n}",
    "{\n\"classname\" \"item_cells\"\n\"origin\" \"32 32 0\"\n}",
    "{\n\"classname\" \"item_artifact_super_damage\"\n\"origin\" \"64 32 0\"\n}",
];
