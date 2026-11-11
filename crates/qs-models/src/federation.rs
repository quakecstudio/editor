// SPDX-License-Identifier: GPL-3.0-or-later

//! Workload identity federation (P4-38, AGT-11): a short-lived token from the user's identity
//! provider exchanged for a short-lived API token, in place of a stored API key. Read on
//! 2026-11-11 from each provider's documentation:
//!
//! - **Anthropic** (platform.claude.com/docs/en/build-with-claude/workload-identity-federation):
//!   the JWT is posted to `POST /v1/oauth/token` with `"grant_type":
//!   "urn:ietf:params:oauth:grant-type:jwt-bearer"`, the JWT as `assertion`, and
//!   `federation_rule_id`, `organization_id`, `service_account_id` and `workspace_id`; the
//!   answer's `access_token` is sent as `authorization: Bearer`, and "The response's expires_in
//!   is the token's lifetime in seconds; re-exchange before it elapses". Its SDKs refresh "at
//!   expiry minus 120 seconds".
//! - **OpenAI** (developers.openai.com/api/reference/workload-identity-federation): a JSON post
//!   to `https://auth.openai.com/oauth/token` with `grant_type`
//!   `urn:ietf:params:oauth:grant-type:token-exchange`, `subject_token_type`
//!   `urn:ietf:params:oauth:token-type:jwt`, the JWT as `subject_token`, `identity_provider_id`
//!   and `service_account_id`; the answer's `access_token` is a Bearer token, `expires_at` its
//!   "Absolute expiration as a Unix timestamp in seconds", and "Token exchange doesn't return a
//!   refresh token. To renew, repeat the exchange".

use std::path::PathBuf;
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{ProviderConfig, Secret, Transport};

/// OpenAI's token endpoint for workload identity.
pub const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";

/// How long before its expiry an access token is exchanged again: the 120 seconds Anthropic's
/// SDKs allow, for both providers, which leaves room for the clocks to differ.
pub const RENEW_BEFORE_SECS: u64 = 120;

/// A provider reached through workload identity: where the identity token comes from, and
/// what the provider needs to exchange it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Federated {
    /// Where the identity provider's token is read from each time it is exchanged.
    pub identity: IdentityToken,
    /// The provider's identifiers for the exchange.
    pub exchange: Exchange,
}

/// Where the identity provider's JWT comes from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "lowercase")]
pub enum IdentityToken {
    /// A file the identity provider keeps current, such as a projected service account token.
    File {
        /// The file.
        path: PathBuf,
    },
    /// A command that prints the token, such as a cloud CLI's token command.
    Command {
        /// The program.
        program: String,
        /// Its arguments.
        #[serde(default)]
        args: Vec<String>,
    },
}

/// What a provider needs to exchange an identity token.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase")]
pub enum Exchange {
    /// Anthropic's federation rule and the identity it acts as.
    Anthropic {
        /// The federation rule (`fdrl_...`).
        federation_rule_id: String,
        /// The organization's id.
        organization_id: String,
        /// The service account (`svac_...`).
        service_account_id: String,
        /// The workspace (`wrkspc_...`), when the rule names one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        workspace_id: Option<String>,
    },
    /// OpenAI's workload identity provider and service account.
    OpenAi {
        /// The workload identity provider configured for the issuer.
        identity_provider_id: String,
        /// The project service account.
        service_account_id: String,
    },
}

/// An access token the exchange gave, and when to exchange again, in Unix seconds.
#[derive(Clone)]
pub(crate) struct AccessToken {
    pub(crate) token: Secret,
    pub(crate) renew_at: u64,
}

impl IdentityToken {
    /// The token, read now.
    ///
    /// # Errors
    ///
    /// Why it could not be read, or that it is empty.
    pub fn read(&self) -> Result<String, String> {
        let text = match self {
            Self::File { path } => std::fs::read_to_string(path).map_err(|e| {
                format!("the identity token {} cannot be read: {e}", path.display())
            })?,
            Self::Command { program, args } => {
                let output = Command::new(program).args(args).output().map_err(|e| {
                    format!("the identity token command {program} did not run: {e}")
                })?;
                if !output.status.success() {
                    return Err(format!(
                        "the identity token command {program} failed ({}): {}",
                        output.status,
                        String::from_utf8_lossy(&output.stderr).trim()
                    ));
                }
                String::from_utf8_lossy(&output.stdout).into_owned()
            }
        };
        let token = text.trim();
        if token.is_empty() {
            return Err("the identity token is empty".to_owned());
        }
        Ok(token.to_owned())
    }
}

/// Exchanges `federated`'s identity token for an access token from `config`'s provider over
/// `transport`, at Unix time `now`.
pub(crate) fn exchange<T: Transport>(
    transport: &T,
    config: &ProviderConfig,
    federated: &Federated,
    now: u64,
) -> Result<AccessToken, String> {
    let jwt = federated.identity.read()?;
    let (url, body) = match &federated.exchange {
        Exchange::Anthropic {
            federation_rule_id,
            organization_id,
            service_account_id,
            workspace_id,
        } => {
            let mut body = json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:jwt-bearer",
                "assertion": jwt,
                "federation_rule_id": federation_rule_id,
                "organization_id": organization_id,
                "service_account_id": service_account_id,
            });
            if let (Some(workspace), Some(object)) = (workspace_id, body.as_object_mut()) {
                object.insert("workspace_id".to_owned(), json!(workspace));
            }
            (format!("{}/v1/oauth/token", config.base_url()), body)
        }
        Exchange::OpenAi {
            identity_provider_id,
            service_account_id,
        } => (
            OPENAI_TOKEN_URL.to_owned(),
            json!({
                "grant_type": "urn:ietf:params:oauth:grant-type:token-exchange",
                "subject_token_type": "urn:ietf:params:oauth:token-type:jwt",
                "subject_token": jwt,
                "identity_provider_id": identity_provider_id,
                "service_account_id": service_account_id,
            }),
        ),
    };
    let answer = transport
        .post_json(&url, &[], &body)
        .map_err(|error| format!("the token exchange failed: {error}"))?;
    let token = answer
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or("the token exchange answered no access_token")?;
    let expires = answer
        .get("expires_at")
        .and_then(Value::as_u64)
        .or_else(|| {
            answer
                .get("expires_in")
                .and_then(Value::as_u64)
                .map(|lifetime| now.saturating_add(lifetime))
        })
        .ok_or("the token exchange answered no expires_in")?;
    Ok(AccessToken {
        token: Secret::new(token.to_owned()),
        renew_at: expires.saturating_sub(RENEW_BEFORE_SECS),
    })
}
