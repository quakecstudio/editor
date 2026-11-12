// SPDX-License-Identifier: GPL-3.0-or-later

#![expect(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code: a failed unwrap or index is a failed test"
)]

use super::{EntityClass, Fgd, Found, KeyType, Shape, parse_quaked, quaked_comments};
use crate::{Db, FileId, ModuleInput};

/// The definitions every fixture declares, as id's `defs.qc` does.
const DEFS: &str = "entity self; .string classname, model, target, targetname, noise;\n\
    .float spawnflags, wait, speed, health, lip, dmg; .vector angles, origin, mins, maxs, size;\n\
    .void() think, use; .entity enemy; float time;\n\
    void(entity e, string m) setmodel = #3; void(entity e, vector a, vector b) setsize = #4;\n";

/// A module for [`classes`]: its files after [`DEFS`], in compilation order, and the dialect.
/// The default is one empty file in the vanilla dialect.
#[derive(Clone, Copy, Debug)]
struct ModuleSpec<'a> {
    files: &'a [&'a str],
    dialect: qc_syntax::Dialect,
}

impl Default for ModuleSpec<'_> {
    fn default() -> Self {
        Self {
            files: &[""],
            dialect: qc_syntax::Dialect::default(),
        }
    }
}

/// The classes of the module `spec` describes, [`DEFS`] its first file.
fn classes(spec: &ModuleSpec<'_>) -> Vec<EntityClass> {
    let mut db = Db::new();
    db.set_dialect(spec.dialect);
    let mut input = ModuleInput::default();
    for (index, text) in std::iter::once(&DEFS).chain(spec.files).enumerate() {
        let id = FileId::new(index);
        db.set_file(id, format!("f{index}.qc").into(), text.as_bytes().to_vec());
        input.compiled.push(id);
    }
    db.entity_classes(&input)
}

/// The classes of one file after [`DEFS`].
fn classes_of(source: &str) -> Vec<EntityClass> {
    classes(&ModuleSpec {
        files: &[source],
        ..ModuleSpec::default()
    })
}

/// The class named `name`.
fn class<'a>(classes: &'a [EntityClass], name: &str) -> &'a EntityClass {
    let found = classes.iter().find(|class| class.name == name);
    assert!(found.is_some(), "no class {name} in {classes:?}");
    found.unwrap()
}

/// The names of a class's keys.
fn keys(class: &EntityClass) -> Vec<&str> {
    class.keys.iter().map(|key| key.name.as_str()).collect()
}

// spec EDICT-8.6: the engine calls the function a classname names with `self` the entity, so a
// documented void() function is a class of its name; one with parameters is not
#[test]
fn a_documented_function_is_a_class() {
    let found = classes_of(
        "/*QUAKED info_null (0 0.5 0) (-4 -4 -4) (4 4 4)\nUsed as a positional target.\n*/\n\
         void() info_null = { };\n\
         /*QUAKED not_a_spawn (0 0 0) (0 0 0) (0 0 0)\n*/\nvoid(float f) not_a_spawn = { };\n",
    );
    assert_eq!(found.len(), 1, "{found:?}");
    let null = class(&found, "info_null");
    assert_eq!(null.found, Found::Documented);
    assert_eq!(null.shape, Shape::Point);
    assert_eq!(null.color, Some([0.0, 0.5, 0.0]));
    assert_eq!(null.size, Some([[-4.0; 3], [4.0; 3]]));
    assert_eq!(null.doc, ["Used as a positional target."]);
}

// spec EDICT-9.1: a function nothing names is a class when it uses a field of self, but not one
// of the engine's other entry points, a function another names, or one that uses no field
#[test]
fn an_unreferenced_function_using_self_is_a_class() {
    let found = classes_of(
        "void() helper = { self.wait = 1; };\n\
         void() trap_thing = { helper(); self.speed = 2; };\n\
         void() StartFrame = { self.wait = 0; };\n\
         void() idle = { };\n",
    );
    let names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["trap_thing"]);
    assert_eq!(found[0].found, Found::Unreferenced);
}

// No spec statement: P5-03: a key is a field the class reads from self through its own body
// and its callees, with the field's type; a vector's component is the vector; a field it
// writes while spawning before any read is not a key, nor are classname, functions and entities
#[test]
fn keys_are_the_fields_a_class_reads() {
    let found = classes_of(
        ".float angles_y;\n\
         void() read_more = { if (self.noise) self.wait = self.angles_y; };\n\
         /*QUAKED func_thing (0 0 0) ?\n*/\n\
         void() func_thing = { self.health = 10; if (self.health) self.dmg = 1; \
         if (self.classname == \"x\") self.enemy = self.enemy; read_more(); \
         self.speed = self.lip; };\n",
    );
    let thing = class(&found, "func_thing");
    assert_eq!(keys(thing), ["angles", "lip", "noise"]);
    let types: Vec<KeyType> = thing.keys.iter().map(|key| key.ty).collect();
    assert_eq!(types, [KeyType::Vector, KeyType::Float, KeyType::String]);
    assert_eq!(thing.shape, Shape::Brush);
}

// No spec statement: P5-03: a function a class stores in self, its think or use, runs later; a
// field it reads is a key unless the class writes it anywhere
#[test]
fn a_stored_function_reads_keys_the_class_never_writes() {
    let found = classes_of(
        "void() later = { if (self.target) self.wait = self.wait + 1; };\n\
         /*QUAKED monster_thing (1 0 0) (-16 -16 -24) (16 16 32)\n*/\n\
         void() monster_thing = { self.think = later; self.use = later; };\n",
    );
    assert_eq!(keys(class(&found, "monster_thing")), ["target"]);
}

// No spec statement: P5-03: a constant the spawn function stores into a field it read first is
// the key's default, as a map writes it; a store with no read before it is no default
#[test]
fn a_constant_given_after_a_read_is_the_default() {
    let found = classes_of(
        "/*QUAKED func_door2 (0 .5 .8) ?\n*/\n\
         void() func_door2 = { if (!self.speed) self.speed = 100; \
         if (!self.noise) self.noise = \"doors/a.wav\"; \
         if (!self.angles) self.angles = '0 90 0'; if (!self.lip) self.lip = 0.5; \
         self.dmg = 2; if (self.dmg) self.wait = 1; };\n",
    );
    let door = class(&found, "func_door2");
    let defaults: Vec<(&str, Option<&str>)> = door
        .keys
        .iter()
        .map(|key| (key.name.as_str(), key.default.as_deref()))
        .collect();
    assert_eq!(
        defaults,
        [
            ("angles", Some("0 90 0")),
            ("lip", Some("0.5")),
            ("noise", Some("doors/a.wav")),
            ("speed", Some("100")),
        ]
    );
}

// spec EDICT-8.4: spawnflags is a float the class masks with a power of two; a constant
// names its flag, and the comment names the flags at their bits from 1
#[test]
fn spawnflags_come_from_masks_and_the_comment() {
    let found = classes_of(
        "float DOOR_TOGGLE = 32;\n\
         /*QUAKED func_flags (0 0 0) ? START_OPEN x DONT_LINK\n*/\n\
         void() func_flags = { if (self.spawnflags & DOOR_TOGGLE) self.wait = 1; \
         if (self.spawnflags & 1) self.wait = 2; if (self.spawnflags & 16) self.wait = 3; \
         if (self.spawnflags & 3) self.wait = 4; };\n",
    );
    let flags: Vec<(u32, Option<&str>)> = class(&found, "func_flags")
        .spawnflags
        .iter()
        .map(|flag| (flag.value, flag.name.as_deref()))
        .collect();
    assert_eq!(
        flags,
        [
            (1, Some("START_OPEN")),
            (4, Some("DONT_LINK")),
            (16, None),
            (32, Some("DOOR_TOGGLE")),
        ]
    );
}

// spec BUILTIN-9.2, BUILTIN-9.4: the model a class passes to setmodel is its model, the box it
// passes to setsize its size when the comment gives none, and setmodel(self, self.model) makes
// a brush entity; mins, maxs and size, which the builtins set, are not keys
#[test]
fn setmodel_and_setsize_give_the_model_and_size() {
    let found = classes_of(
        "void() item_thing = { setmodel(self, \"progs/thing.mdl\"); \
         setsize(self, '-16 -16 0', '16 16 56'); if (self.mins) self.wait = 1; };\n\
         void() func_wall2 = { setmodel(self, self.model); };\n",
    );
    let item = class(&found, "item_thing");
    assert_eq!(item.model.as_deref(), Some("progs/thing.mdl"));
    assert_eq!(item.size, Some([[-16.0, -16.0, 0.0], [16.0, 16.0, 56.0]]));
    assert_eq!(item.shape, Shape::Point);
    assert!(keys(item).is_empty(), "{:?}", item.keys);
    let wall = class(&found, "func_wall2");
    assert_eq!(wall.shape, Shape::Brush);
    assert_eq!(wall.size, None);
}

// No spec statement: P5-03: a comment's colour, box, brush mark, flags and text; the rerelease
// writes /* and QUAKED on separate lines; a comment that is not QUAKED is skipped
#[test]
fn quaked_comments_are_read_as_written() {
    let text = b"/* a plain comment */\n\
        /*\nQUAKED func_wall (0 .5 .8) ?\nThis is just a solid wall\n\nif not inhibited\n\n*/\n\
        /*QUAKED light (0 1 0) (-8 -8 -8) (8 8 8) START_OFF\nNon-displayed light.\n*/";
    let found = quaked_comments(text);
    let names: Vec<&str> = found.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["func_wall", "light"]);
    let wall = &found[0].1;
    assert!(wall.brush);
    assert_eq!(wall.color, Some([0.0, 0.5, 0.8]));
    assert_eq!(
        wall.doc,
        ["This is just a solid wall", "", "if not inhibited"]
    );
    let light = &found[1].1;
    assert_eq!(light.flags, ["START_OFF"]);
    assert_eq!(light.size, Some([[-8.0; 3], [8.0; 3]]));
}

// No spec statement: P5-03: a comment with no name, or a box that is not three numbers, keeps
// what it can read
#[test]
fn malformed_comments() {
    assert_eq!(parse_quaked(""), None);
    assert_eq!(parse_quaked("   \n"), None);
    let (name, partial) = parse_quaked(" thing (1 0) (0 0 0) (1 1 1)").unwrap();
    assert_eq!(name, "thing");
    assert_eq!(partial.color, None);
    assert_eq!(partial.size, None);
    let (_, unclosed) = parse_quaked(" thing (1 0 0").unwrap();
    assert_eq!(unclosed.color, None);
    let unended = quaked_comments(b"/*QUAKED open (1 1 1) (0 0 0) (1 1 1)\ntext");
    assert_eq!(unended.len(), 1);
}

// No spec statement: P5-03: the FGD has one class each, point or solid, with its colour, size,
// model, flags, keys with their editor types and defaults, and quotes made single
#[test]
fn the_fgd_lists_each_class() {
    let found = classes_of(
        "float START_OPEN = 1;\n\
         /*QUAKED func_door3 (0 .5 .8) ? x\nA \"door\".\n\tIt opens.\n*/\n\
         void() func_door3 = { if (self.spawnflags & START_OPEN) self.wait = 1; \
         if (!self.speed) self.speed = 100; if (self.target) self.wait = 2; \
         if (self.targetname) self.wait = 3; };\n\
         /*QUAKED item_x (0 0 1) (0 0 0) (32 32 32)\n*/\n\
         void() item_x = { setmodel(self, \"progs/x.mdl\"); };\n",
    );
    assert_eq!(
        Fgd(&found).to_string(),
        "@SolidClass color(0 128 204) = func_door3 : \"A 'door'. It opens.\"\n\
         [\n\
         \tspawnflags(flags) =\n\
         \t[\n\
         \t\t1 : \"START_OPEN\" : 0\n\
         \t]\n\
         \tspeed(float) : \"speed\" : \"100\"\n\
         \ttarget(target_destination) : \"target\"\n\
         \ttargetname(target_source) : \"targetname\"\n\
         ]\n\
         \n\
         @PointClass color(0 0 255) size(0 0 0, 32 32 32) model({ \"path\": \"progs/x.mdl\" }) \
         = item_x : \"\"\n\
         [\n\
         ]\n"
    );
    assert_eq!(Fgd(&[]).to_string(), "");
}

// No spec statement: P5-03: under FTEQCC the classes are read the same way
#[test]
fn fteqcc_modules_give_the_same_classes() {
    let source = "/*QUAKED func_y (0 0 0) ?\n*/\n\
        void() func_y = { if (!self.wait) self.wait = 3; };\n";
    let vanilla = classes_of(source);
    let fte = classes(&ModuleSpec {
        files: &[source],
        dialect: qc_syntax::Dialect::Fteqcc,
    });
    assert_eq!(keys(&vanilla[0]), keys(&fte[0]));
    assert_eq!(fte[0].keys[0].default.as_deref(), Some("3"));
}

// No spec statement: P5-03: a module with no spawn function has no classes
#[test]
fn no_spawn_functions_no_classes() {
    assert!(classes(&ModuleSpec::default()).is_empty());
}

/// The targeting keys of a module of [`DEFS`] and `files`.
fn targeting(files: &[&str]) -> Vec<String> {
    let mut db = Db::new();
    let mut input = ModuleInput::default();
    for (index, text) in std::iter::once(&DEFS).chain(files).enumerate() {
        let id = FileId::new(index);
        db.set_file(id, format!("f{index}.qc").into(), text.as_bytes().to_vec());
        input.compiled.push(id);
    }
    db.targeting_keys(&input)
}

// spec MAP-8.8: a key fires entities when the code passes its field to find as the value to
// match against targetname, from self or another entity, or compares it with targetname by ==
// or !=; a comparison with a global, or find over another field, makes no key
#[test]
fn targeting_keys_are_read_from_the_code() {
    let code = ".string killtarget, target2, pathtarget, netname; string string_null;\n\
        entity other;\n\
        entity(entity start, .string fld, string match) find = #18;\n\
        void() use_targets = { local entity t;\n\
            t = find (world, targetname, self.killtarget);\n\
            t = find (t, targetname, other.target);\n\
            if (t.targetname == self.target2) t = world;\n\
            if (self.pathtarget != t.targetname) t = world;\n\
            if (self.targetname != string_null) t = world;\n\
            t = find (world, netname, self.noise);\n\
        };\n";
    assert_eq!(
        targeting(&[code]),
        ["killtarget", "pathtarget", "target", "target2"]
    );
    assert!(targeting(&[""]).is_empty());
}
