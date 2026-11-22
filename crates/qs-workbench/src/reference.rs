// SPDX-License-Identifier: GPL-3.0-or-later

//! The reference an agent reads before it works (L-45, L-46, AGT-9): `project.reference`
//! answers one topic of `docs/agent-reference/`, the same text the built-in agent's prompt is
//! built from and more of it, so an agent that reaches the IDE over MCP, with no prompt of
//! ours, knows the language in the project's dialect, the engine's entity model and how a
//! monster, a weapon, an item and a trigger are written as well as the project's own code
//! lets it.

use qs_commands::{Command, Outcome, RegisterError, Registry};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::WorkbenchError;

/// The id of `project.reference`.
pub const REFERENCE: &str = "project.reference";

/// A topic of the agent reference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Topic {
    /// The language, how a mod is laid out and run, and its pitfalls.
    Quakec,
    /// The vanilla dialect in full: QuakeC as id's compiler reads it, and what a model gets
    /// wrong in it.
    Vanilla,
    /// The rerelease's dialect: what FTEQCC adds for id's VM, and what stays as in vanilla.
    Rerelease,
    /// The FTEQCC dialect in full: integers, pointers, structs, classes, the preprocessor.
    Fteqcc,
    /// What the engine calls, how an entity moves and collides, the builtins by what they
    /// are for, what the language does that C does not, and debugging.
    Entities,
    /// How a monster is written, from id's own, and where a new one must be added.
    Monsters,
    /// How a weapon is written, from id's own, and the six places a new one is added.
    Weapons,
    /// How an item is written, from id's own: its spawn function and its touch.
    Items,
    /// How a trigger is written, from id's own, and how targets are fired.
    Triggers,
    /// What a model is, a mod's own models, and how to recolour, scale and reframe one.
    Models,
    /// The `.map` format, units, the entities a map needs, textures and leaks.
    Maps,
    /// How to work in QuakeC Studio: finding code, changing it, tests and scenarios.
    Workflow,
}

impl Topic {
    /// Every topic, in the order to read them.
    pub const ALL: [Self; 12] = [
        Self::Quakec,
        Self::Vanilla,
        Self::Rerelease,
        Self::Fteqcc,
        Self::Entities,
        Self::Monsters,
        Self::Weapons,
        Self::Items,
        Self::Triggers,
        Self::Models,
        Self::Maps,
        Self::Workflow,
    ];

    /// The topic's text, as `docs/agent-reference/` holds it.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Quakec => include_str!("../../../docs/agent-reference/quakec.md"),
            Self::Vanilla => include_str!("../../../docs/agent-reference/language-vanilla.md"),
            Self::Rerelease => {
                include_str!("../../../docs/agent-reference/language-rerelease.md")
            }
            Self::Fteqcc => include_str!("../../../docs/agent-reference/language-fteqcc.md"),
            Self::Entities => include_str!("../../../docs/agent-reference/entities.md"),
            Self::Monsters => include_str!("../../../docs/agent-reference/monsters.md"),
            Self::Weapons => include_str!("../../../docs/agent-reference/weapons.md"),
            Self::Items => include_str!("../../../docs/agent-reference/items.md"),
            Self::Triggers => include_str!("../../../docs/agent-reference/triggers.md"),
            Self::Models => include_str!("../../../docs/agent-reference/models.md"),
            Self::Maps => include_str!("../../../docs/agent-reference/maps.md"),
            Self::Workflow => include_str!("../../../docs/agent-reference/workflow.md"),
        }
    }
}

/// The parameters of `project.reference`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferenceParams {
    /// The topic to read.
    pub topic: Topic,
}

/// What `project.reference` answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Reference {
    /// The topic.
    pub topic: Topic,
    /// Its text, in Markdown.
    pub text: String,
}

/// `project.reference`.
struct ProjectReference;

impl<C: 'static> Command<C> for ProjectReference {
    const ID: &'static str = REFERENCE;
    const ACCESS: qs_commands::Access = qs_commands::Access::Read;
    const SUMMARY: &'static str = "The reference to read before writing or debugging QuakeC or \
        building a map, by topic: quakec (the language in short, how a mod runs, pitfalls); \
        the dialect in full, with the mistakes a model makes in it: vanilla (id's QuakeC) \
        when project.info says vanilla; when it says fteqcc, rerelease (what FTEQCC adds for \
        id's VM), and fteqcc too (integers, pointers, classes) where the project's own code \
        uses them; entities (what the engine calls, movetype and solid, the builtins \
        by purpose, debugging); monsters, weapons, items and triggers (how each is written, \
        from id's own, and every place a new one is added); models (what a model is, and how \
        to recolour, scale and reframe one); maps; workflow (tests and scenarios). Written \
        from id's QuakeC and the engine's specs; a mod's own code comes first where it \
        differs.";
    type Params = ReferenceParams;
    type Output = Reference;
    type Error = WorkbenchError;

    fn run(
        &self,
        _context: &mut C,
        params: ReferenceParams,
    ) -> Result<Outcome<Reference, C>, WorkbenchError> {
        Ok(Outcome::outside_history(Reference {
            topic: params.topic,
            text: params.topic.text().to_owned(),
        }))
    }
}

/// Registers `project.reference`.
///
/// # Errors
///
/// [`RegisterError`] when its id is taken.
pub(crate) fn register_reference<C: 'static>(
    registry: &mut Registry<C>,
) -> Result<(), RegisterError> {
    registry.register(ProjectReference)
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        reason = "test code: a failed unwrap is a failed test"
    )]

    use qs_commands::{Registry, Session};
    use serde_json::json;

    use super::{REFERENCE, Reference, Topic, register_reference};
    use crate::map_compile::tests::Context;
    use crate::testing::tests::{ProjectSpec, project};
    use crate::testing::{RunTestsParams, Verdict};
    use crate::{register, register_preview};

    // No spec statement: L-45, AGT-9: every topic of the agent reference is served whole, by
    // its name, with no project open; a topic there is none of is refused
    #[test]
    fn each_topic_is_served() {
        let mut registry = Registry::new();
        register_reference(&mut registry).unwrap();
        let mut session = Session::new((), registry);
        for topic in Topic::ALL {
            let name = serde_json::to_value(topic).unwrap();
            let served: Reference = serde_json::from_value(
                session
                    .execute(REFERENCE, json!({ "topic": name }))
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(served.topic, topic);
            assert_eq!(served.text, topic.text());
            assert!(served.text.starts_with("# "), "{topic:?} has a title");
        }
        assert!(
            session
                .execute(REFERENCE, json!({ "topic": "nosuch" }))
                .is_err()
        );
        assert!(session.execute(REFERENCE, json!({})).is_err());
    }

    // No spec statement: L-45: the reference names only tools there are: each `word.word` in
    // code marks that starts with a kind of command is a command's id
    #[test]
    fn the_reference_names_real_commands() {
        let mut registry: Registry<Context> = Registry::new();
        register(&mut registry).unwrap();
        register_preview(&mut registry).unwrap();
        // The kinds of command there are: what each id starts with, up to its first dot.
        let kinds: std::collections::BTreeSet<String> = registry
            .commands()
            .filter_map(|info| info.id().as_str().split_once('.'))
            .map(|(kind, _)| format!("{kind}."))
            .collect();
        assert!(kinds.len() > 10, "{kinds:?}");
        let mut named = 0;
        for topic in Topic::ALL {
            for word in topic.text().split('`').skip(1).step_by(2) {
                let id = word.split_whitespace().next().unwrap_or_default();
                // A file of a mod is named as a command is: `ai.qc`, `progs.src`.
                let file = id.ends_with(".qc") || id.ends_with(".src") || id.ends_with(".md");
                if kinds.iter().any(|kind| id.starts_with(kind.as_str())) && !file {
                    assert!(registry.get(id).is_some(), "{topic:?} names {id}");
                    named += 1;
                }
            }
        }
        assert!(
            named > 20,
            "the check read the commands the reference names: {named}"
        );
    }

    /// What the `vanilla` topic says of the operators, conditions, scopes and calls, as
    /// tests that fail by calling `error`.
    const VANILLA: &str = "\
float hits;\n\
void(string e) error = #10;\n\
float(float v) side = { hits = hits + 1; return v; };\n\
float(float a, float b) first = { return a; };\n\
void() test_and_runs_both_sides = { hits = 0; if (0 && side(1)) hits = 100; if (hits != 1) error(\"&& skipped its right side\"); };\n\
void() test_or_runs_both_sides = { hits = 0; if (1 || side(1)) hits = hits + 0; if (hits != 1) error(\"|| skipped its right side\"); };\n\
void() test_bits_bind_as_times = { local float a; a = 1 + 1 & 1; if (a != 2) error(\"a + b & c\"); };\n\
void() test_not_takes_the_comparison = { local float a, b, t; a = 2; b = 1; t = 0; if (!a == b) t = 1; if (!t) error(\"!a == b\"); };\n\
void() test_assign_binds_tighter_than_and = { local float a; a = 5; a = 0 && 1; if (a != 0) error(\"a = b && c\"); a = 7 && 0; if (a != 7) error(\"a = b && c stores b\"); };\n\
void() test_an_empty_string_literal_is_true = { local string str; local float t; str = \"\"; t = 0; if (str) t = 1; if (!t) error(\"if on an empty literal\"); if (!(!str)) error(\"! on an empty literal\"); };\n\
void() test_a_vector_tests_its_x = { local vector v; local float t; v = '0 0 1'; t = 0; if (v) t = 1; if (t) error(\"if (v)\"); if (v == '0 0 0') error(\"==\"); };\n\
void() test_two_calls_share_the_return = { local float r; r = side(2) + side(4); if (r != 8) error(\"f() + g()\"); };\n\
void() test_a_call_in_a_later_argument_overwrites = { local float r; r = first(side(5), 1); if (r != 5) error(\"the first argument's call\"); r = first(4, side(9)); if (r != 9) error(\"the later argument's call\"); };\n\
void() scoped = { local float hits; hits = 7; };\n\
void() test_a_local_is_the_global = { hits = 3; scoped(); if (hits != 7) error(\"a local hid a global\"); };\n";

    /// What the `rerelease` topic says is added, and what stays as in vanilla.
    const RERELEASE: &str = "\
float hits;\n\
void(string e) error = #10;\n\
float side(float v) { hits = hits + 1; return v; }\n\
void test_and_runs_both_sides() { hits = 0; if (0 && side(1)) hits = 100; if (hits != 1) error(\"&& skipped its right side\"); }\n\
void test_or_runs_both_sides() { hits = 0; if (1 || side(1)) hits = hits + 0; if (hits != 1) error(\"|| skipped its right side\"); }\n\
void test_bits_bind_as_times() { float a = 1 + 1 & 1; if (a != 2) error(\"a + b & c\"); }\n\
void test_the_ternary_binds_tighter_than_and() { float t = 0 && 1 ? 5 : 7; if (t != 0) error(\"a && b ? c : d\"); }\n\
float first(float a, float b) { return a; }\n\
void test_a_call_in_a_later_argument_keeps_the_earlier() { if (first(4, side(9)) != 4) error(\"the earlier argument\"); }\n\
void test_not_takes_the_comparison() { float a = 2; float b = 1; float t = 0; if (!a == b) t = 1; if (!t) error(\"!a == b\"); }\n\
void test_an_empty_string_literal_is_true() { string s = \"\"; float t = 0; if (s) t = 1; if (!t) error(\"if on an empty literal\"); }\n\
void test_a_vector_tests_every_part() { vector v = '0 0 1'; float t = 0; if (v) t = 1; if (!t) error(\"if (v)\"); }\n\
void scoped() { float hits = 5; hits = 7; }\n\
void test_a_local_hides_a_global() { hits = 3; scoped(); if (hits != 3) error(\"a local wrote the global\"); }\n\
void test_calls_keep_their_values() { if (side(2) + side(4) != 6) error(\"f() + g()\"); if (side(side(2) + 1) + side(4) != 7) error(\"nested\"); }\n\
void test_minus() { float a = 4; float b = -a; if (b != -4) error(\"-a\"); if (a-1 != 3) error(\"a-1\"); }\n\
void test_numbers_are_floats() { float h = 7 / 2; if (h != 3.5) error(\"7 / 2\"); if (7 % 3 != 1) error(\"%\"); if (.5 + 1.0f != 1.5) error(\"literals\"); }\n\
void test_loops_and_arrays() { float sum = 0; float xs[] = { 1, 2, 3 }; for (float i = 0; i < xs.length; i++) { if (i == 1) continue; sum += xs[i]; } if (sum != 4) error(\"for\"); }\n\
void test_switch_and_ternary() { float r = (hits > 100) ? 1 : 2; switch (r) { case 1: error(\"one\"); break; case 2: r = 9; break; default: error(\"default\"); } if (r != 9) error(\"switch\"); }\n\
void test_vector_members() { vector v = '1 2 3'; v.z *= 2; if (v_z != 6) error(\"v.z\"); if (v.x + v.y != 3) error(\"v.x\"); }\n\
enum { EA, EB, EC };\n\
const float SPEED = 320;\n\
var float count = 0;\n\
void test_constants() { if (EC != 2) error(\"enum\"); count = count + SPEED; if (count != 320) error(\"var\"); }\n\
float with_default(float a, float b = 4) { return a + b; }\n\
void test_default_parameter() { if (with_default(1) != 5) error(\"default\"); if (with_default(1, 2) != 3) error(\"given\"); }\n";

    /// What the `fteqcc` topic says compiles and runs for id's VM, beyond the rerelease's.
    const FTEQCC: &str = "\
float hits;\n\
void(string e) error = #10;\n\
class Barrel : entity { float charge; virtual void() Explode; void() Barrel; };\n\
void() Barrel::Explode = { charge = 0; };\n\
void() Barrel::Barrel = { charge = 3; };\n\
enumflags { FA, FB, FC };\n\
struct Pair { float a; vector b; };\n\
struct Pair made = { .a = 1, .b = '0 0 1' };\n\
void test_enumflags_double() { if (FA + FB + FC != 7) error(\"enumflags\"); }\n\
void test_a_struct_at_file_level() { if (made.a != 1) error(\"a\"); if (made.b != '0 0 1') error(\"b\"); }\n\
void test_a_local_struct_assigned() { struct Pair p; p.a = 2; if (p.a != 2) error(\"p.a\"); }\n\
void test_a_local_struct_in_braces() { struct Pair p = { .b = '1 2 3' }; if (p.a != 0) error(\"p.a\"); if (p.b != '1 2 3') error(\"p.b\"); struct Pair q = { 4, '0 0 5' }; vector w = q.b; if (q.a + w.z != 9) error(\"q\"); }\n\
float set_then_go_on() { return = 6; hits = hits + 1; }\n\
void test_return_equals() { hits = 0; if (set_then_go_on() != 6) error(\"return =\"); if (hits != 1) error(\"went on\"); }\n\
void test_a_vector_of_expressions() { float n = 3; vector v = [1, 2, n]; if (v.z != 3) error(\"[x, y, z]\"); }\n\
void test_adjacent_strings_join() { string s = \"a\" \"b\"; if (s != \"ab\") error(\"strings\"); }\n\
void test_the_short_ternary() { float e = 0 ?: 7; if (e != 7) error(\"a ?: b\"); }\n\
void test_if_not() { float e = 1; if not (e) error(\"if not\"); }\n\
void test_plain_numbers_are_floats() { float h = 5 / 2; if (h != 2.5) error(\"5 / 2\"); }\n\
void test_a_case_range() { float r = 0; switch (5) { case 1 .. 3: r = 1; break; case 4 .. 6: r = 2; break; } if (r != 2) error(\"case a .. b\"); }\n";

    /// A project file of the FTEQCC dialect for the rerelease's engine.
    const FOR_IDS_VM: &str = "[project]\ndialect = \"fteqcc\"\ntarget = \"rerelease\"\n\n\
                              [[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n";

    /// A project file of the FTEQCC dialect for FTE's engine, whose opcodes the program may use.
    const FOR_FTE: &str = "[project]\ndialect = \"fteqcc\"\ntarget = \"fte\"\n\n\
                           [[module]]\nkind = \"ssqc\"\nsrc = \"progs.src\"\n";

    /// Runs the `test_` functions of `code` in a project with `manifest` for its
    /// `quakec.toml`, and returns each that did not pass.
    fn failures(name: &str, manifest: Option<&'static str>, code: &str) -> Vec<String> {
        let spec = ProjectSpec {
            code: code.to_owned(),
            manifest,
            ..ProjectSpec::default()
        };
        let (root, mut workbench) = project(name, &spec);
        let run = workbench.run_tests(&RunTestsParams::default()).unwrap();
        let _gone = std::fs::remove_dir_all(&root);
        assert!(run.tests.len() > 8, "{name}: {} tests ran", run.tests.len());
        run.tests
            .into_iter()
            .filter(|test| test.verdict != Verdict::Passed)
            .map(|test| format!("{}: {:?}", test.name, test.message))
            .collect()
    }

    // No spec statement: L-46, AGT-9: what the dialect topics say of the operators, the
    // conditions, the scopes and the calls is what this compiler's programs do when run, in
    // the vanilla dialect and in FTEQCC's for id's VM
    #[test]
    fn the_dialect_topics_hold_when_run() {
        assert_eq!(
            failures("reference-vanilla", None, VANILLA),
            Vec::<String>::new()
        );
        assert_eq!(
            failures("reference-rerelease", Some(FOR_IDS_VM), RERELEASE),
            Vec::<String>::new()
        );
        assert_eq!(
            failures("reference-fteqcc", Some(FOR_IDS_VM), FTEQCC),
            Vec::<String>::new()
        );
        // The same program for FTE's engine: what the topic says holds on either target.
        assert_eq!(
            failures("reference-fteqcc-fte", Some(FOR_FTE), FTEQCC),
            Vec::<String>::new()
        );
    }
}
