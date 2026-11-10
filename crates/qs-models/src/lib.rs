// SPDX-License-Identifier: GPL-3.0-or-later

//! The agent's model providers (P4-07, AGT-6, PRIN-9): Anthropic's Messages API, any
//! OpenAI-compatible chat completions endpoint, and a local Ollama, each a [`qs_agent::Model`]
//! the agent loop runs. The person picks the provider and the model; nothing is sent anywhere
//! else, and with Ollama nothing leaves the machine.
//!
//! API keys live in the OS keychain ([`Keychain`]: macOS Keychain, Windows Credential Manager,
//! the Secret Service on Linux), never in [`ProviderConfig`], which is what a settings file
//! keeps. A key is a [`Secret`] in memory: it has no `Display` or `Serialize`, and its `Debug`
//! prints `Secret(<redacted>)`, so neither a log line, a settings file nor a crash record can
//! hold it. Only the request's header ever reads it.

mod anthropic;
mod http;
mod keys;
mod openai;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use qs_agent::{Message, Model, Reply, ToolSpec};

pub use crate::http::{Header, Http, Transport, TransportError};
pub use crate::keys::{KeyError, KeyStore, Keychain, MemoryKeys};

/// An API key. It is never printed, logged or written: [`Secret::expose`] is the only way to
/// read it, and only a request's header does.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// A secret holding `key`.
    #[must_use]
    pub const fn new(key: String) -> Self {
        Self(key)
    }

    /// The key, for the one header that sends it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Which API a provider speaks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Anthropic's Messages API.
    Anthropic,
    /// An OpenAI-compatible chat completions endpoint (OpenAI, or any server speaking it).
    OpenAiCompatible,
    /// A local Ollama, over its OpenAI-compatible endpoint, with no key.
    Ollama,
}

impl ProviderKind {
    /// The base URL used when the configuration names none.
    #[must_use]
    pub const fn default_base_url(self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAiCompatible => "https://api.openai.com/v1",
            // Ollama's documented local server (docs.ollama.com, OpenAI compatibility).
            Self::Ollama => "http://localhost:11434/v1",
        }
    }

    /// Whether the provider needs an API key.
    #[must_use]
    pub const fn needs_key(self) -> bool {
        match self {
            Self::Anthropic | Self::OpenAiCompatible => true,
            Self::Ollama => false,
        }
    }
}

/// How hard an Anthropic model thinks: the Messages API's `output_config.effort`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// `low`.
    Low,
    /// `medium`.
    Medium,
    /// `high`, the default here: agentic coding repays it.
    #[default]
    High,
    /// `xhigh`.
    Xhigh,
    /// `max`.
    Max,
}

/// The provider and model the agent runs on, as a settings file keeps it: no key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// The API.
    pub kind: ProviderKind,
    /// The model's id at the provider.
    pub model: String,
    /// Where the API is, when not [`ProviderKind::default_base_url`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// The most tokens one reply may have.
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// How hard an Anthropic model thinks.
    #[serde(default)]
    pub effort: Effort,
}

/// The reply limit when the configuration names none.
const fn default_max_tokens() -> u32 {
    16_000
}

impl ProviderConfig {
    /// `kind` running `model` at its default URL.
    #[must_use]
    pub fn new(kind: ProviderKind, model: &str) -> Self {
        Self {
            kind,
            model: model.to_owned(),
            base_url: None,
            max_tokens: default_max_tokens(),
            effort: Effort::default(),
        }
    }

    /// Anthropic's current default model, `claude-opus-5-5`.
    #[must_use]
    pub fn anthropic() -> Self {
        Self::new(ProviderKind::Anthropic, "claude-opus-5-5")
    }

    /// The base URL, without a trailing `/`.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.base_url
            .as_deref()
            .unwrap_or(self.kind.default_base_url())
            .trim_end_matches('/')
    }

    /// The keychain entry the provider's key is under: one per provider and URL, so two
    /// OpenAI-compatible servers keep two keys.
    #[must_use]
    pub fn key_name(&self) -> String {
        match self.kind {
            ProviderKind::Anthropic => "anthropic".to_owned(),
            ProviderKind::OpenAiCompatible => format!("openai-compatible {}", self.base_url()),
            ProviderKind::Ollama => format!("ollama {}", self.base_url()),
        }
    }
}

/// Why a provider could not be used.
#[derive(Debug)]
pub enum ProviderError {
    /// The provider needs a key and the keychain has none for it.
    NoKey(String),
    /// The keychain could not be read.
    Keys(KeyError),
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoKey(name) => write!(f, "no API key is stored for {name}"),
            Self::Keys(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ProviderError {}

/// A provider's model, answering the agent loop over `T`.
pub struct ProviderModel<T> {
    config: ProviderConfig,
    key: Option<Secret>,
    transport: T,
    /// Each reply's message exactly as the provider sent it, in order: the transcript's n-th
    /// assistant message is sent back as the n-th of these, so what the provider adds (an
    /// Anthropic model's thinking blocks) goes back unchanged.
    sent: Vec<Value>,
}

impl<T: Transport> ProviderModel<T> {
    /// The model `config` names over `transport`, with `key` when the provider needs one.
    ///
    /// # Errors
    ///
    /// [`ProviderError::NoKey`] when the provider needs a key and `key` is `None`.
    pub fn new(
        config: ProviderConfig,
        key: Option<Secret>,
        transport: T,
    ) -> Result<Self, ProviderError> {
        if config.kind.needs_key() && key.is_none() {
            return Err(ProviderError::NoKey(config.key_name()));
        }
        Ok(Self {
            config,
            key,
            transport,
            sent: Vec::new(),
        })
    }

    /// The configuration.
    #[must_use]
    pub const fn config(&self) -> &ProviderConfig {
        &self.config
    }
}

impl ProviderModel<Http> {
    /// The model `config` names over HTTPS, its key read from `keys`.
    ///
    /// # Errors
    ///
    /// [`ProviderError`] when the key is needed and missing, or the keychain fails.
    pub fn connect(config: ProviderConfig, keys: &dyn KeyStore) -> Result<Self, ProviderError> {
        let key = if config.kind.needs_key() {
            keys.get(&config.key_name()).map_err(ProviderError::Keys)?
        } else {
            None
        };
        Self::new(config, key, Http::new())
    }
}

impl<T: Transport> Model for ProviderModel<T> {
    fn reply(
        &mut self,
        system: &str,
        tools: &[ToolSpec],
        transcript: &[Message],
    ) -> Result<Reply, String> {
        let (url, headers, body) = match self.config.kind {
            ProviderKind::Anthropic => anthropic::request(
                &self.config,
                self.key.as_ref(),
                system,
                tools,
                transcript,
                &self.sent,
            ),
            ProviderKind::OpenAiCompatible | ProviderKind::Ollama => openai::request(
                &self.config,
                self.key.as_ref(),
                system,
                tools,
                transcript,
                &self.sent,
            ),
        };
        tracing::debug!(provider = ?self.config.kind, model = %self.config.model, "model request");
        let answer = self
            .transport
            .post_json(&url, &headers, &body)
            .map_err(|error| error.to_string())?;
        let (reply, raw) = match self.config.kind {
            ProviderKind::Anthropic => anthropic::reply(&answer, tools)?,
            ProviderKind::OpenAiCompatible | ProviderKind::Ollama => openai::reply(&answer, tools)?,
        };
        self.sent.push(raw);
        Ok(reply)
    }
}

/// A command id as a tool name the providers take: letters, digits, `_` and `-` only (OpenAI's
/// `FunctionObject.name`: "Must be a-z, A-Z, 0-9, or contain underscores and dashes, with a
/// maximum length of 64"), so `edit.apply` is sent as `edit_apply`.
fn tool_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The command a tool name the model called stands for, among `tools`; the name itself when
/// none matches, which the command session then refuses as unknown.
fn command_of(name: &str, tools: &[ToolSpec]) -> String {
    tools
        .iter()
        .find(|tool| tool_name(&tool.name) == name)
        .map_or_else(|| name.to_owned(), |tool| tool.name.clone())
}

/// A tool's result as the text a provider takes.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}
