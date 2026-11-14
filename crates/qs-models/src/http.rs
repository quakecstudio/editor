// SPDX-License-Identifier: GPL-3.0-or-later

//! How a request reaches a provider: JSON posted over HTTP(S).

use std::time::Duration;

use serde_json::Value;

use crate::Secret;

/// How long one request may take, connection to last byte: a long reply of a thinking model
/// takes minutes.
const TIMEOUT: Duration = Duration::from_mins(10);

/// One request header. A key travels as a [`Secret`], so a header printed for debugging shows
/// none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Header {
    /// A header whose value may be shown.
    Plain(&'static str, String),
    /// A header carrying a key.
    Secret(&'static str, Secret),
}

impl Header {
    /// The header's name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Plain(name, _) | Self::Secret(name, _) => name,
        }
    }

    /// The value sent.
    #[must_use]
    pub fn value(&self) -> &str {
        match self {
            Self::Plain(_, value) => value,
            Self::Secret(_, secret) => secret.expose(),
        }
    }
}

/// Why a request got no answer. None of its messages holds a header's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// The provider could not be reached or the connection failed.
    Connect(String),
    /// The provider answered with an error status; the body is its message.
    Status {
        /// The HTTP status.
        code: u16,
        /// What it said.
        body: String,
    },
    /// The answer is not JSON.
    Body(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connect(why) => write!(f, "the model provider could not be reached: {why}"),
            Self::Status { code, body } => {
                write!(f, "the model provider answered {code}: {body}")
            }
            Self::Body(why) => write!(f, "the model provider's answer is not JSON: {why}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// Posts JSON and reads JSON back.
pub trait Transport {
    /// Posts `body` to `url` with `headers`, answering the JSON the provider sent.
    ///
    /// # Errors
    ///
    /// [`TransportError`] when there is no answer, an error status or no JSON.
    fn post_json(
        &self,
        url: &str,
        headers: &[Header],
        body: &Value,
    ) -> Result<Value, TransportError>;

    /// Posts `body` to `url` with `headers` and reads the answer as server-sent events,
    /// calling `on_data` with each event's `data` line as it arrives; `[DONE]` ends the stream.
    ///
    /// # Errors
    ///
    /// [`TransportError`] when there is no answer or an error status.
    fn post_stream(
        &self,
        url: &str,
        headers: &[Header],
        body: &Value,
        on_data: &mut dyn FnMut(&str),
    ) -> Result<(), TransportError>;
}

/// HTTP and HTTPS over `ureq`, TLS by rustls with the webpki roots.
pub struct Http {
    agent: ureq::Agent,
}

impl Http {
    /// A client whose requests may take up to ten minutes.
    #[must_use]
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .build();
        Self {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    /// Sends `body` to `url` with `headers`: the response, or the error status with its body.
    fn send(
        &self,
        url: &str,
        headers: &[Header],
        body: &Value,
    ) -> Result<ureq::http::Response<ureq::Body>, TransportError> {
        let mut request = self
            .agent
            .post(url)
            .header("content-type", "application/json");
        for header in headers {
            request = request.header(header.name(), header.value());
        }
        let mut response = request
            .send(body.to_string())
            .map_err(|error| TransportError::Connect(error.to_string()))?;
        let code = response.status().as_u16();
        if response.status().is_success() {
            return Ok(response);
        }
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| TransportError::Connect(error.to_string()))?;
        Err(TransportError::Status { code, body: text })
    }
}

impl Transport for Http {
    fn post_stream(
        &self,
        url: &str,
        headers: &[Header],
        body: &Value,
        on_data: &mut dyn FnMut(&str),
    ) -> Result<(), TransportError> {
        use std::io::BufRead as _;
        let mut response = self.send(url, headers, body)?;
        let reader = std::io::BufReader::new(response.body_mut().as_reader());
        for line in reader.lines() {
            let line = line.map_err(|error| TransportError::Connect(error.to_string()))?;
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim_start();
            if data == "[DONE]" {
                break;
            }
            on_data(data);
        }
        Ok(())
    }

    fn post_json(
        &self,
        url: &str,
        headers: &[Header],
        body: &Value,
    ) -> Result<Value, TransportError> {
        let mut response = self.send(url, headers, body)?;
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|error| TransportError::Connect(error.to_string()))?;
        serde_json::from_str(&text).map_err(|error| TransportError::Body(error.to_string()))
    }
}
