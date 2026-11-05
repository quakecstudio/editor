// SPDX-License-Identifier: GPL-3.0-or-later

//! BRIDGE-2 and BRIDGE-4: the session a host serves: `initialize` first, the protocol
//! version, the capabilities, each command's parameters checked against its schema, and
//! the command run by the host.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use jsonschema::{Draft, Retrieve, Uri, Validator};
use serde_json::{Value, json};

use crate::message::{ErrorCode, Id, Message, RpcError};
use crate::schemas::SCHEMAS;

/// The protocol version this crate speaks, `major.minor` (BRIDGE-2.4).
pub const BRIDGE_PROTOCOL_VERSION: (u32, u32) = (1, 0);

/// Every command, and whether a capability flags it (BRIDGE-2.3, BRIDGE-4.1).
pub const BRIDGE_COMMANDS: [(&str, bool); 18] = [
    ("initialize", false),
    ("shutdown", false),
    ("inspect", false),
    ("reload", true),
    ("breakpoint/set", true),
    ("breakpoint/clear", true),
    ("step", true),
    ("console", true),
    ("pause", true),
    ("resume", true),
    ("field/set", true),
    ("pick", true),
    ("entity/spawn", true),
    ("entity/remove", true),
    ("entity/edit", true),
    ("player/spawn_at", true),
    ("screenshot", true),
    ("evaluate", true),
];

/// Whether `method` is a command a capability flags (BRIDGE-2.3); `false` for one that is
/// not a command.
#[must_use]
pub fn is_flagged(method: &str) -> bool {
    BRIDGE_COMMANDS
        .iter()
        .any(|&(name, flagged)| flagged && name == method)
}

/// The capability-flagged commands a host accepts (BRIDGE-2.3).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities(BTreeSet<String>);

impl Capabilities {
    /// The commands of `methods` that a capability flags.
    #[must_use]
    pub fn of<'a>(methods: impl IntoIterator<Item = &'a str>) -> Self {
        Self(
            methods
                .into_iter()
                .filter(|method| is_flagged(method))
                .map(str::to_owned)
                .collect(),
        )
    }

    /// Whether the host accepts `method`.
    #[must_use]
    pub fn accepts(&self, method: &str) -> bool {
        self.0.contains(method)
    }

    /// One boolean per capability-flagged command, `initialize`'s `capabilities`.
    fn to_json(&self) -> Value {
        let flags: serde_json::Map<String, Value> = BRIDGE_COMMANDS
            .iter()
            .filter(|(_, flagged)| *flagged)
            .map(|&(name, _)| (name.to_owned(), json!(self.accepts(name))))
            .collect();
        Value::Object(flags)
    }
}

/// What a host says of itself in `initialize`'s result (BRIDGE-2.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostInfo {
    /// Its name.
    pub name: String,
    /// Its version.
    pub version: String,
    /// The engine it runs.
    pub engine: String,
}

/// The preview host as the session drives it.
pub trait Host {
    /// Its name, version and engine.
    fn info(&self) -> HostInfo;

    /// The capability-flagged commands it accepts.
    fn capabilities(&self) -> Capabilities;

    /// Runs `method` with `params`, which have passed their schema (BRIDGE-4.2).
    ///
    /// # Errors
    ///
    /// The command's error (BRIDGE-7).
    fn call(&mut self, method: &str, params: &Value) -> Result<Value, RpcError>;

    /// Stops the game, for `shutdown` (BRIDGE-2.5).
    fn shutdown(&mut self);
}

/// What the session answers a message with.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    /// The response to send, if any.
    pub response: Option<Message>,
    /// Whether the connection closes after it (BRIDGE-2.4).
    pub close: bool,
}

impl Reply {
    const fn none() -> Self {
        Self {
            response: None,
            close: false,
        }
    }

    fn to(id: Id, outcome: Result<Value, RpcError>) -> Self {
        Self {
            response: Some(Message::Response {
                id: Some(id),
                outcome,
            }),
            close: false,
        }
    }
}

/// One client's session (BRIDGE-2).
#[derive(Clone, Debug, Default)]
pub struct Session {
    initialized: bool,
}

impl Session {
    /// A session not yet initialized.
    #[must_use]
    pub const fn new() -> Self {
        Self { initialized: false }
    }

    /// Whether `initialize` has been answered, so events may be sent (BRIDGE-2.2).
    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// The reply to `message`, running any command on `host` (BRIDGE-2.1 to BRIDGE-2.5,
    /// BRIDGE-4.2, BRIDGE-6.2).
    pub fn handle(&mut self, host: &mut dyn Host, message: Message) -> Reply {
        let Message::Request { id, method, params } = message else {
            // Notifications from the client, and its responses, ask nothing of the host.
            return Reply::none();
        };
        if method == "initialize" {
            return self.initialize(host, id, &params);
        }
        if !self.initialized {
            let error = RpcError::new(ErrorCode::NotInitialized, "initialize comes first");
            return Reply::to(id, Err(error));
        }
        if !BRIDGE_COMMANDS.iter().any(|&(name, _)| name == method) {
            let error = RpcError::new(ErrorCode::MethodNotFound, format!("no method {method}"));
            return Reply::to(id, Err(error));
        }
        if is_flagged(&method) && !host.capabilities().accepts(&method) {
            let error = RpcError::new(ErrorCode::Unsupported, format!("{method} is not supported"));
            return Reply::to(id, Err(error));
        }
        if let Err(error) = check(&format!("{}.params", method.replace('/', ".")), &params) {
            return Reply::to(id, Err(error));
        }
        if method == "shutdown" {
            host.shutdown();
            return Reply::to(id, Ok(Value::Null));
        }
        Reply::to(id, host.call(&method, &params))
    }

    /// `initialize` (BRIDGE-2.1, BRIDGE-2.2, BRIDGE-2.4).
    fn initialize(&mut self, host: &dyn Host, id: Id, params: &Value) -> Reply {
        if let Err(error) = check("initialize.params", params) {
            return Reply::to(id, Err(error));
        }
        let asked = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .and_then(version);
        let (major, minor) = BRIDGE_PROTOCOL_VERSION;
        if asked.is_none_or(|(asked, _)| asked != major) {
            let error = RpcError::new(
                ErrorCode::IncompatibleVersion,
                format!("this host speaks version {major}.{minor}"),
            );
            return Reply {
                response: Some(Message::Response {
                    id: Some(id),
                    outcome: Err(error),
                }),
                close: true,
            };
        }
        self.initialized = true;
        let info = host.info();
        Reply::to(
            id,
            Ok(json!({
                "protocolVersion": format!("{major}.{minor}"),
                "host": { "name": info.name, "version": info.version, "engine": info.engine },
                "capabilities": host.capabilities().to_json(),
            })),
        )
    }
}

/// The notification of event `method` with `params` (BRIDGE-3.1).
#[must_use]
pub fn event(method: &str, params: Value) -> Message {
    Message::Notification {
        method: method.to_owned(),
        params,
    }
}

/// `major.minor` in decimal (BRIDGE-2.4).
fn version(text: &str) -> Option<(u32, u32)> {
    let (major, minor) = text.split_once('.')?;
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// Checks `value` against the schema `name`, `-32602` when it fails (BRIDGE-4.2).
fn check(name: &str, value: &Value) -> Result<(), RpcError> {
    let validator = validator(name)
        .ok_or_else(|| RpcError::new(ErrorCode::InternalError, format!("no schema {name}")))?;
    match validator.validate(value) {
        Ok(()) => Ok(()),
        Err(error) => Err(RpcError::new(ErrorCode::InvalidParams, error.to_string())),
    }
}

/// The schemas' common base, which a `$ref` names them under.
const BASE: &str = "https://quakecstudio.com/spec/bridge-schema/";

/// Resolves a `$ref` to one of the built-in schemas.
struct Builtin;

impl Retrieve for Builtin {
    fn retrieve(
        &self,
        uri: &Uri<String>,
    ) -> Result<Value, Box<dyn core::error::Error + Send + Sync>> {
        let name = uri
            .as_str()
            .strip_prefix(BASE)
            .and_then(|file| file.strip_suffix(".json"))
            .ok_or_else(|| format!("{uri} is not a bridge schema"))?;
        let text = SCHEMAS
            .iter()
            .find(|(schema, _)| *schema == name)
            .map(|(_, text)| *text)
            .ok_or_else(|| format!("no bridge schema {name}"))?;
        Ok(serde_json::from_str(text)?)
    }
}

/// The validator of schema `name`, built once.
fn validator(name: &str) -> Option<&'static Validator> {
    static VALIDATORS: OnceLock<Vec<(&'static str, Validator)>> = OnceLock::new();
    let all = VALIDATORS.get_or_init(|| {
        SCHEMAS
            .iter()
            .filter_map(|&(name, text)| {
                let schema: Value = serde_json::from_str(text).ok()?;
                let validator = jsonschema::options()
                    .with_draft(Draft::Draft202012)
                    .with_retriever(Builtin)
                    .build(&schema)
                    .ok()?;
                Some((name, validator))
            })
            .collect()
    });
    all.iter()
        .find(|(schema, _)| *schema == name)
        .map(|(_, validator)| validator)
}

#[cfg(test)]
pub(crate) fn schema_checks(name: &str, value: &Value) -> bool {
    check(name, value).is_ok()
}

#[cfg(test)]
mod tests;
