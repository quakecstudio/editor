// SPDX-License-Identifier: GPL-3.0-or-later

//! Which tools the agent is offered and whether each call runs, asks or is refused (P4-30,
//! UI-22, PRIN-4).
//!
//! The model is Zed's agent: a profile picks the tools (Zed's Ask, Write and Minimal, here
//! Ask, Write and Full) and tool permissions decide each call, with patterns per tool and the
//! same precedence (Zed's docs, `ai/tool-permissions.md`, "Rule Precedence"): a `deny` pattern,
//! then `ask` (Zed's `always_confirm`), then `allow`, then the tool's default, then the global
//! default. Here the profile decides last, when no rule does. Zed matches regular expressions;
//! these patterns are globs over paths, which is what a QuakeC project's rules are about.

use std::path::Path;

use qs_settings::{AgentProfile, Decision, ToolPermissions};
use serde_json::Value;

use crate::{ToolCall, collect_paths, lexical};

/// The tools that only read the project, the game or the settings: the Ask profile's, and
/// allowed in every profile.
pub const READ_TOOLS: [&str; 22] = [
    "checkpoint.list",
    "compile",
    "edit.staged",
    "engine.builtins",
    "engine.inspect",
    "engine.screenshot",
    "file.read",
    "preview.poll",
    "project.check",
    "project.files",
    "project.info",
    "project.search",
    "project.symbols",
    "project.test",
    "scenario.run",
    "settings.get",
    "settings.list",
    "symbol.complete",
    "symbol.definition",
    "symbol.hover",
    "symbol.references",
    "vm.run",
];

/// The tools that change the project's code, which the Write profile runs unasked: its edits
/// are staged for review and its build only writes the compiler's output.
pub const EDIT_TOOLS: [&str; 2] = ["edit.apply", "project.build"];

/// What decides a run's tool calls: the profile and the rules over it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Permissions {
    /// The profile.
    pub profile: AgentProfile,
    /// The rules, from the settings.
    pub rules: ToolPermissions,
}

/// What a call came to and why, in words the model and the modder read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// Whether it runs, asks or is refused.
    pub decision: Decision,
    /// What decided it.
    pub why: String,
}

impl Permissions {
    /// Every profile and no rules.
    #[must_use]
    pub fn of(profile: AgentProfile) -> Self {
        Self {
            profile,
            rules: ToolPermissions::default(),
        }
    }

    /// Whether the profile offers tool `name` to the model.
    #[must_use]
    pub fn offers(&self, name: &str) -> bool {
        match self.profile {
            AgentProfile::Ask => READ_TOOLS.contains(&name),
            AgentProfile::Write | AgentProfile::Full => true,
        }
    }

    /// What the profile does with a call of tool `name` when no rule decides it.
    fn fallback(&self, name: &str) -> Decision {
        let reads = READ_TOOLS.contains(&name);
        let edits = EDIT_TOOLS.contains(&name);
        match self.profile {
            AgentProfile::Full => Decision::Allow,
            AgentProfile::Write if reads || edits => Decision::Allow,
            AgentProfile::Write => Decision::Ask,
            AgentProfile::Ask if reads => Decision::Allow,
            AgentProfile::Ask => Decision::Deny,
        }
    }

    /// What happens to `call` in the project at `project`: each of its subjects decided on
    /// its own ([`subjects`]), the strictest winning.
    #[must_use]
    pub fn decide(&self, call: &ToolCall, project: &Path) -> Verdict {
        subjects(call, project)
            .iter()
            .map(|subject| self.decide_one(&call.name, subject))
            .max_by_key(|verdict| verdict.decision)
            .unwrap_or_else(|| self.decide_one(&call.name, ""))
    }

    /// What happens to a call of tool `name` on `subject`.
    fn decide_one(&self, name: &str, subject: &str) -> Verdict {
        let rule = self.rules.tools.get(name);
        let matched = |patterns: Option<&Vec<String>>| {
            patterns
                .into_iter()
                .flatten()
                .find(|pattern| glob(pattern, subject))
                .cloned()
        };
        let verdict = |decision, why: String| Verdict { decision, why };
        if let Some(pattern) = matched(rule.map(|r| &r.deny)) {
            return verdict(
                Decision::Deny,
                format!("the rule deny `{pattern}` of {name} matches {subject}"),
            );
        }
        if let Some(pattern) = matched(rule.map(|r| &r.ask)) {
            return verdict(
                Decision::Ask,
                format!("the rule ask `{pattern}` of {name} matches {subject}"),
            );
        }
        if let Some(pattern) = matched(rule.map(|r| &r.allow)) {
            return verdict(
                Decision::Allow,
                format!("the rule allow `{pattern}` of {name} matches {subject}"),
            );
        }
        if let Some(decision) = rule.and_then(|r| r.default) {
            return verdict(decision, format!("{name}'s default is {}", word(decision)));
        }
        if let Some(decision) = self.rules.default {
            return verdict(
                decision,
                format!("the tool permissions' default is {}", word(decision)),
            );
        }
        let decision = self.fallback(name);
        verdict(
            decision,
            format!(
                "the {} profile {} {name}",
                profile_word(self.profile),
                match decision {
                    Decision::Allow => "allows",
                    Decision::Ask => "asks before",
                    Decision::Deny => "does not allow",
                }
            ),
        )
    }

    /// Allows every later call of tool `name` on `subject` unasked (Always allow): an `allow`
    /// pattern that matches it alone, `*` and `?` in it matching only themselves.
    pub fn allow_always(&mut self, name: &str, subject: &str) {
        let pattern = escape(subject);
        let rule = self.rules.tools.entry(name.to_owned()).or_default();
        if !rule.allow.contains(&pattern) {
            rule.allow.push(pattern);
        }
    }
}

/// What a decision is called in the settings.
const fn word(decision: Decision) -> &'static str {
    match decision {
        Decision::Allow => "allow",
        Decision::Ask => "ask",
        Decision::Deny => "deny",
    }
}

/// What a profile is called in the panel.
const fn profile_word(profile: AgentProfile) -> &'static str {
    match profile {
        AgentProfile::Ask => "Ask",
        AgentProfile::Write => "Write",
        AgentProfile::Full => "Full",
    }
}

/// What the patterns of `call` are matched against: each path it names, relative to
/// `project` with `/` between its parts (absolute when it is outside), or, when it names none,
/// its arguments as JSON.
#[must_use]
pub fn subjects(call: &ToolCall, project: &Path) -> Vec<String> {
    let mut paths = Vec::new();
    collect_paths(&call.arguments, &mut paths);
    if paths.is_empty() {
        return vec![match &call.arguments {
            Value::Object(object) if object.is_empty() => String::new(),
            other => other.to_string(),
        }];
    }
    let project = lexical(project);
    paths
        .iter()
        .map(|path| {
            let full = lexical(&project.join(path));
            full.strip_prefix(&project).map_or_else(
                |_| full.to_string_lossy().replace('\\', "/"),
                |inside| {
                    inside
                        .components()
                        .map(|part| part.as_os_str().to_string_lossy())
                        .collect::<Vec<_>>()
                        .join("/")
                },
            )
        })
        .collect()
}

/// Whether `text` matches `pattern`, without regard to case: `*` is any run of characters,
/// `?` any one, and `\` makes the next character match only itself.
#[must_use]
pub fn glob(pattern: &str, text: &str) -> bool {
    let pattern: Vec<Token> = tokens(&pattern.to_lowercase());
    let text: Vec<char> = text.to_lowercase().chars().collect();
    // The classic two-pointer match, which backs up to the last `*` on a mismatch.
    let (mut p, mut t) = (0_usize, 0_usize);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match (pattern.get(p), text.get(t)) {
            (Some(Token::Any), _) => {
                star = Some((p, t));
                p = p.saturating_add(1);
            }
            (Some(Token::One), Some(_)) => {
                p = p.saturating_add(1);
                t = t.saturating_add(1);
            }
            (Some(Token::Char(c)), Some(d)) if c == d => {
                p = p.saturating_add(1);
                t = t.saturating_add(1);
            }
            _ => match star {
                Some((star_p, star_t)) => {
                    p = star_p.saturating_add(1);
                    t = star_t.saturating_add(1);
                    star = Some((star_p, t));
                }
                None => return false,
            },
        }
    }
    pattern
        .get(p..)
        .is_some_and(|rest| rest.iter().all(|token| *token == Token::Any))
}

/// One piece of a pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token {
    /// `*`.
    Any,
    /// `?`.
    One,
    /// A character that matches itself.
    Char(char),
}

/// `pattern` in pieces, each `\` making the next character match itself.
fn tokens(pattern: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = pattern.chars();
    while let Some(c) = chars.next() {
        out.push(match c {
            '*' => Token::Any,
            '?' => Token::One,
            '\\' => Token::Char(chars.next().unwrap_or('\\')),
            c => Token::Char(c),
        });
    }
    out
}

/// `text` as a pattern that matches only itself.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '*' | '?' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::indexing_slicing,
        reason = "test code: a failed index is a failed test"
    )]

    use std::path::Path;

    use qs_settings::{AgentProfile, Decision, ToolRule};
    use serde_json::json;

    use super::{Permissions, glob, subjects};
    use crate::ToolCall;

    /// A call of `name` with `arguments`.
    fn call(name: &str, arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "1".to_owned(),
            name: name.to_owned(),
            arguments,
        }
    }

    /// The Write profile with `rule` for `file.read` and the global default `default`.
    fn with_rule(rule: ToolRule, default: Option<Decision>) -> Permissions {
        let mut permissions = Permissions::of(AgentProfile::Write);
        permissions.rules.default = default;
        permissions.rules.tools.insert("file.read".to_owned(), rule);
        permissions
    }

    /// What `permissions` decides for `file.read` of `path` in `/p`.
    fn read(permissions: &Permissions, path: &str) -> Decision {
        permissions
            .decide(&call("file.read", json!({ "path": path })), Path::new("/p"))
            .decision
    }

    // No spec statement: P4-30, Zed's rule precedence: deny over ask over allow over the tool's
    // default over the global default over the profile
    #[test]
    fn rules_are_decided_in_zeds_order() {
        let all = |pattern: &str| vec![pattern.to_owned()];
        let rule = ToolRule {
            default: Some(Decision::Deny),
            allow: all("*.qc"),
            ask: all("src/*"),
            deny: all("src/secret*"),
        };
        let permissions = with_rule(rule.clone(), Some(Decision::Allow));
        assert_eq!(
            read(&permissions, "src/secret.qc"),
            Decision::Deny,
            "deny wins"
        );
        assert_eq!(
            read(&permissions, "src/world.qc"),
            Decision::Ask,
            "ask over allow"
        );
        assert_eq!(read(&permissions, "world.qc"), Decision::Allow, "allow");
        assert_eq!(
            read(&permissions, "notes.txt"),
            Decision::Deny,
            "the tool's default"
        );
        let global = with_rule(
            ToolRule {
                default: None,
                ..rule
            },
            Some(Decision::Ask),
        );
        assert_eq!(
            read(&global, "notes.txt"),
            Decision::Ask,
            "the global default"
        );
        let profile = with_rule(ToolRule::default(), None);
        assert_eq!(
            read(&profile, "notes.txt"),
            Decision::Allow,
            "the profile reads"
        );
        let why = profile
            .decide(&call("file.read", json!({ "path": "a" })), Path::new("/p"))
            .why;
        assert_eq!(why, "the Write profile allows file.read");
    }

    // No spec statement: P4-30, each profile's own choice: Ask offers and allows reading only,
    // Write allows reads and edits and asks before the rest, Full allows everything
    #[test]
    fn profiles_offer_and_decide_their_tools() {
        let decide = |profile, name: &str| {
            Permissions::of(profile)
                .decide(&call(name, json!({})), Path::new("/p"))
                .decision
        };
        let ask = Permissions::of(AgentProfile::Ask);
        assert!(ask.offers("file.read") && !ask.offers("edit.apply"));
        assert_eq!(decide(AgentProfile::Ask, "edit.apply"), Decision::Deny);
        assert!(Permissions::of(AgentProfile::Write).offers("preview.play"));
        assert_eq!(decide(AgentProfile::Write, "edit.apply"), Decision::Allow);
        assert_eq!(decide(AgentProfile::Write, "preview.play"), Decision::Ask);
        assert_eq!(decide(AgentProfile::Write, "settings.set"), Decision::Ask);
        assert_eq!(decide(AgentProfile::Full, "preview.play"), Decision::Allow);
    }

    // No spec statement: P4-30, a call naming several paths is decided by the strictest, each
    // path relative to the project; one naming none by its arguments
    #[test]
    fn the_strictest_subject_decides() {
        let permissions = with_rule(
            ToolRule {
                deny: vec!["progs.src".to_owned()],
                ..ToolRule::default()
            },
            None,
        );
        let both = call(
            "file.read",
            json!({ "edits": [{ "path": "/p/world.qc" }, { "path": "progs.src" }] }),
        );
        assert_eq!(subjects(&both, Path::new("/p")), ["world.qc", "progs.src"]);
        assert_eq!(
            permissions.decide(&both, Path::new("/p")).decision,
            Decision::Deny
        );
        let outside = call("file.read", json!({ "path": "../elsewhere/x.qc" }));
        assert_eq!(subjects(&outside, Path::new("/p")), ["/elsewhere/x.qc"]);
        let console = call("engine.console", json!({ "command": "map e1m1" }));
        assert_eq!(
            subjects(&console, Path::new("/p")),
            [r#"{"command":"map e1m1"}"#]
        );
    }

    // No spec statement: P4-30, patterns: `*` any run, `?` one character, case ignored, `\`
    // escapes; Always allow matches only the subject it was given
    #[test]
    fn patterns_match_as_globs() {
        for (pattern, text, matches) in [
            ("*.qc", "src/world.qc", true),
            ("*.qc", "world.qcx", false),
            ("src/?orld.qc", "SRC/World.QC", true),
            ("", "", true),
            ("", "a", false),
            ("*", "", true),
            ("a*b*c", "aXbYbZc", true),
            ("a*b*c", "aXbYbZ", false),
            ("\\*.qc", "*.qc", true),
            ("\\*.qc", "a.qc", false),
        ] {
            assert_eq!(glob(pattern, text), matches, "{pattern} on {text}");
        }
        let mut permissions = Permissions::of(AgentProfile::Write);
        permissions.allow_always("preview.play", "w*.qc");
        permissions.allow_always("preview.play", "w*.qc");
        let rule = &permissions.rules.tools["preview.play"];
        assert_eq!(rule.allow, ["w\\*.qc"], "once, and only itself");
        assert!(glob(&rule.allow[0], "w*.qc") && !glob(&rule.allow[0], "world.qc"));
    }
}
