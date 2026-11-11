// SPDX-License-Identifier: GPL-3.0-or-later

//! `[agent]` of a settings file (P4-30, UI-22): the profile the agent works under and the
//! rules for its tool calls. The types are data; `qs-agent` decides calls with them.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which tools the agent is offered and what it may do with them unasked, as Zed's agent
/// profiles name them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AgentProfile {
    /// Questions only: the tools that read the project and the game, each allowed.
    Ask,
    /// Reads and edits are allowed; anything else that changes the project, the game or the
    /// settings asks first.
    #[default]
    Write,
    /// Every tool, each allowed.
    Full,
}

/// What happens to a tool call.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    /// It runs.
    Allow,
    /// It waits until the modder allows or denies it.
    Ask,
    /// It never runs, and the model reads why.
    Deny,
}

/// The rules for one tool. A pattern matches what a call names, each path relative to the
/// project, or the call's arguments as JSON when it names no path; `*` is any run of
/// characters and `?` any one, without regard to case.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolRule {
    /// What happens when no pattern matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Decision>,
    /// Patterns whose calls run unasked, unless an `ask` or `deny` pattern matches too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allow: Vec<String>,
    /// Patterns whose calls always ask, unless a `deny` pattern matches too.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ask: Vec<String>,
    /// Patterns whose calls never run, whatever else matches.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deny: Vec<String>,
}

/// `agent.tool_permissions`: the rules for each tool, by its command id, and what happens to
/// a call no rule decides, over the profile's own choice.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolPermissions {
    /// What happens to a call of a tool whose rules decide nothing; the profile decides when
    /// unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Decision>,
    /// The rules of each tool, by command id (`edit.apply`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tools: BTreeMap<String, ToolRule>,
}

/// `[agent]` of a settings file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Agent {
    /// The profile.
    #[serde(default)]
    pub profile: Option<AgentProfile>,
    /// The rules for tool calls.
    #[serde(default)]
    pub tool_permissions: Option<ToolPermissions>,
}
