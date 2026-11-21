// SPDX-License-Identifier: GPL-3.0-or-later

//! The reference an agent reads before it works (L-45, AGT-9): `project.reference` answers
//! one topic of `docs/agent-reference/`, the same text the built-in agent's prompt is built
//! from and more of it, so an agent that reaches the IDE over MCP, with no prompt of ours,
//! knows the language, the engine's entity model and how a monster is written as well as the
//! project's own code lets it.

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
    /// What the engine calls, how an entity moves and collides, the builtins by what they
    /// are for, what the language does that C does not, and debugging.
    Entities,
    /// How a monster is written, from id's own, and where a new one must be added.
    Monsters,
    /// The `.map` format, units, the entities a map needs, textures and leaks.
    Maps,
    /// How to work in QuakeC Studio: finding code, changing it, tests and scenarios.
    Workflow,
}

impl Topic {
    /// Every topic, in the order to read them.
    pub const ALL: [Self; 5] = [
        Self::Quakec,
        Self::Entities,
        Self::Monsters,
        Self::Maps,
        Self::Workflow,
    ];

    /// The topic's text, as `docs/agent-reference/` holds it.
    #[must_use]
    pub const fn text(self) -> &'static str {
        match self {
            Self::Quakec => include_str!("../../../docs/agent-reference/quakec.md"),
            Self::Entities => include_str!("../../../docs/agent-reference/entities.md"),
            Self::Monsters => include_str!("../../../docs/agent-reference/monsters.md"),
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
        building a map, by topic: quakec (the language, how a mod runs, pitfalls), entities \
        (what the engine calls, movetype and solid, the builtins by purpose, what QuakeC does \
        that C does not, debugging), monsters (how a monster is written and every place a new \
        one is added), maps, workflow (tests and scenarios). Written from id's QuakeC and the \
        engine's specs; a mod's own code comes first where it differs.";
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
}
