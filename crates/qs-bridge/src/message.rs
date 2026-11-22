// SPDX-License-Identifier: GPL-3.0-or-later

//! BRIDGE-1.3, BRIDGE-1.4, BRIDGE-7: the JSON-RPC 2.0 request, notification and response, and
//! the error codes; and when an edit applies (BRIDGE-4.4).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// A request's id: an integer or a string the sender chooses (BRIDGE-1.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Id {
    /// An integer id.
    Number(i64),
    /// A string id.
    Text(String),
}

impl Id {
    fn to_json(&self) -> Value {
        match self {
            Self::Number(n) => json!(n),
            Self::Text(text) => json!(text),
        }
    }

    fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::Number(n) => n.as_i64().map(Self::Number),
            Value::String(text) => Some(Self::Text(text.clone())),
            _ => None,
        }
    }
}

/// The error codes (BRIDGE-7.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorCode {
    /// `-32700`: the content is not JSON.
    ParseError,
    /// `-32600`: not a JSON-RPC 2.0 object, or a batch.
    InvalidRequest,
    /// `-32601`: an unknown method (BRIDGE-6.2).
    MethodNotFound,
    /// `-32602`: the parameters fail their schema (BRIDGE-4.2).
    InvalidParams,
    /// `-32603`: the host failed in a way the others do not name.
    InternalError,
    /// `-32001`: `initialize` with another major version (BRIDGE-2.11).
    IncompatibleVersion,
    /// `-32002`: a command that needs a paused host (BRIDGE-4.3).
    NotPaused,
    /// `-32003`: a request before `initialize` (BRIDGE-2.1).
    NotInitialized,
    /// `-32004`: an edict number not in use (BRIDGE-4.5).
    NoSuchEdict,
    /// `-32005`: a command whose capability is off (BRIDGE-2.10).
    Unsupported,
    /// `-32006`: a value of the wrong type for a field (BRIDGE-4.4).
    BadValue,
    /// `-32007`: a breakpoint id or line the host cannot resolve (BRIDGE-4.5).
    NoSuchBreakpoint,
    /// `-32008`: a command that changes the world while a frame is stopped at a statement
    /// (BRIDGE-4.6).
    StoppedInFrame,
    /// A code the bridge does not name, which a client reads as the error it is: JSON-RPC 2.0
    /// leaves "the remainder of the space" to the application (section 5.1).
    Other(i64),
}

impl ErrorCode {
    // JSON-RPC's codes, then the host's own in JSON-RPC's reserved range (BRIDGE-7.1).
    const PARSE_ERROR: i64 = -32700;
    const INVALID_REQUEST: i64 = -32600;
    const METHOD_NOT_FOUND: i64 = -32601;
    const INVALID_PARAMS: i64 = -32602;
    const INTERNAL_ERROR: i64 = -32603;
    const INCOMPATIBLE_VERSION: i64 = -32001;
    const NOT_PAUSED: i64 = -32002;
    const NOT_INITIALIZED: i64 = -32003;
    const NO_SUCH_EDICT: i64 = -32004;
    const UNSUPPORTED: i64 = -32005;
    const BAD_VALUE: i64 = -32006;
    const NO_SUCH_BREAKPOINT: i64 = -32007;
    const STOPPED_IN_FRAME: i64 = -32008;

    /// The code's number (BRIDGE-7.1).
    #[must_use]
    pub const fn code(self) -> i64 {
        match self {
            Self::ParseError => Self::PARSE_ERROR,
            Self::InvalidRequest => Self::INVALID_REQUEST,
            Self::MethodNotFound => Self::METHOD_NOT_FOUND,
            Self::InvalidParams => Self::INVALID_PARAMS,
            Self::InternalError => Self::INTERNAL_ERROR,
            Self::IncompatibleVersion => Self::INCOMPATIBLE_VERSION,
            Self::NotPaused => Self::NOT_PAUSED,
            Self::NotInitialized => Self::NOT_INITIALIZED,
            Self::NoSuchEdict => Self::NO_SUCH_EDICT,
            Self::Unsupported => Self::UNSUPPORTED,
            Self::BadValue => Self::BAD_VALUE,
            Self::NoSuchBreakpoint => Self::NO_SUCH_BREAKPOINT,
            Self::StoppedInFrame => Self::STOPPED_IN_FRAME,
            Self::Other(number) => number,
        }
    }

    /// The code numbered `number` (BRIDGE-7.1), [`ErrorCode::Other`] for a number the bridge
    /// does not name.
    #[must_use]
    pub const fn of_code(number: i64) -> Self {
        match number {
            Self::PARSE_ERROR => Self::ParseError,
            Self::INVALID_REQUEST => Self::InvalidRequest,
            Self::METHOD_NOT_FOUND => Self::MethodNotFound,
            Self::INVALID_PARAMS => Self::InvalidParams,
            Self::INTERNAL_ERROR => Self::InternalError,
            Self::INCOMPATIBLE_VERSION => Self::IncompatibleVersion,
            Self::NOT_PAUSED => Self::NotPaused,
            Self::NOT_INITIALIZED => Self::NotInitialized,
            Self::NO_SUCH_EDICT => Self::NoSuchEdict,
            Self::UNSUPPORTED => Self::Unsupported,
            Self::BAD_VALUE => Self::BadValue,
            Self::NO_SUCH_BREAKPOINT => Self::NoSuchBreakpoint,
            Self::STOPPED_IN_FRAME => Self::StoppedInFrame,
            _ => Self::Other(number),
        }
    }
}

/// An error a response carries: its code, a message for the person and optional data
/// (BRIDGE-7.2).
#[derive(Clone, Debug, PartialEq)]
pub struct RpcError {
    /// What went wrong, which a client acts on.
    pub code: ErrorCode,
    /// For the person.
    pub message: String,
    /// What the command's schema may describe.
    pub data: Option<Value>,
}

impl RpcError {
    /// An error of `code` saying `message`, with no data.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    fn to_json(&self) -> Value {
        let mut error = Map::new();
        error.insert("code".to_owned(), json!(self.code.code()));
        error.insert("message".to_owned(), json!(self.message));
        if let Some(data) = &self.data {
            error.insert("data".to_owned(), data.clone());
        }
        Value::Object(error)
    }
}

/// One message (BRIDGE-1.3).
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// A request: `method` and `id`.
    Request {
        /// Its id.
        id: Id,
        /// Its method.
        method: String,
        /// Its parameters, `{}` when it has none.
        params: Value,
    },
    /// A notification: `method` and no `id`.
    Notification {
        /// Its method.
        method: String,
        /// Its parameters, `{}` when it has none.
        params: Value,
    },
    /// A response: `id`, `null` only when the request's could not be read, and its result
    /// or its error.
    Response {
        /// The request's id.
        id: Option<Id>,
        /// The result, or the error.
        outcome: Result<Value, RpcError>,
    },
}

impl Message {
    /// The message `content` holds (BRIDGE-1.3, BRIDGE-1.4).
    ///
    /// # Errors
    ///
    /// The response to send instead: `-32700` for content that is not JSON, `-32600` for a
    /// batch or an object that is no JSON-RPC 2.0 message, each with a `null` id unless the
    /// request's id could be read.
    pub fn parse(content: &[u8]) -> Result<Self, Box<Self>> {
        let invalid = |id: Option<Id>, why: &str| {
            Box::new(Self::Response {
                id,
                outcome: Err(RpcError::new(ErrorCode::InvalidRequest, why)),
            })
        };
        let value: Value = serde_json::from_slice(content).map_err(|error| {
            Box::new(Self::Response {
                id: None,
                outcome: Err(RpcError::new(ErrorCode::ParseError, error.to_string())),
            })
        })?;
        let Value::Object(object) = value else {
            return Err(invalid(
                None,
                "a message is one JSON object; batches are not supported",
            ));
        };
        let id = object.get("id").and_then(Id::from_json);
        if object.get("jsonrpc") != Some(&json!("2.0")) {
            return Err(invalid(id, "jsonrpc is not \"2.0\""));
        }
        if object
            .get("id")
            .is_some_and(|raw| !raw.is_null() && id.is_none())
        {
            return Err(invalid(None, "an id is an integer or a string"));
        }
        let params = match object.get("params") {
            None => json!({}),
            Some(params @ Value::Object(_)) => params.clone(),
            Some(_) => return Err(invalid(id, "params is an object")),
        };
        match (object.get("method"), object.contains_key("id")) {
            (Some(Value::String(method)), true) => {
                if object.contains_key("result") || object.contains_key("error") {
                    return Err(invalid(id, "a request has no result or error"));
                }
                match id {
                    Some(id) => Ok(Self::Request {
                        id,
                        method: method.clone(),
                        params,
                    }),
                    None => Err(invalid(None, "a request's id is not null")),
                }
            }
            (Some(Value::String(method)), false) => Ok(Self::Notification {
                method: method.clone(),
                params,
            }),
            (Some(_), _) => Err(invalid(id, "method is a string")),
            (None, true) => Self::response(&object, id)
                .ok_or_else(|| invalid(None, "a response has exactly one of result and error")),
            (None, false) => Err(invalid(
                None,
                "neither a request, a notification nor a response",
            )),
        }
    }

    /// The response `object` holds, `None` unless it has exactly one of a result and a
    /// well-formed error.
    fn response(object: &Map<String, Value>, id: Option<Id>) -> Option<Self> {
        let outcome = match (object.get("result"), object.get("error")) {
            (Some(result), None) => Ok(result.clone()),
            (None, Some(Value::Object(error))) => {
                let number = error.get("code").and_then(Value::as_i64)?;
                let message = error.get("message").and_then(Value::as_str)?;
                let code = ErrorCode::of_code(number);
                Err(RpcError {
                    code,
                    message: message.to_owned(),
                    data: error.get("data").cloned(),
                })
            }
            _ => return None,
        };
        Some(Self::Response { id, outcome })
    }

    /// The message as its JSON object (BRIDGE-1.4).
    #[must_use]
    pub fn to_json(&self) -> Value {
        match self {
            Self::Request { id, method, params } => {
                json!({ "jsonrpc": "2.0", "id": id.to_json(), "method": method, "params": params })
            }
            Self::Notification { method, params } => {
                json!({ "jsonrpc": "2.0", "method": method, "params": params })
            }
            Self::Response { id, outcome } => {
                let id = id.as_ref().map_or(Value::Null, Id::to_json);
                match outcome {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error.to_json() }),
                }
            }
        }
    }

    /// The message as a frame's content.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.to_json().to_string().into_bytes()
    }
}

/// When an edit applies, as the result of `field/set` and `entity/edit` says (BRIDGE-4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Applied {
    /// Before the next server frame: the host was running.
    Now,
    /// When the game resumes: the host is paused, and the game reads the old value until then.
    OnResume,
}

impl Applied {
    /// The word the result gives.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Now => "now",
            Self::OnResume => "on_resume",
        }
    }

    /// The result that says it: `{"applied": NAME}`.
    #[must_use]
    pub fn to_json(self) -> Value {
        json!({ "applied": self.name() })
    }

    /// What `result` says, `None` when it says neither.
    #[must_use]
    pub fn of_result(result: &Value) -> Option<Self> {
        let name = result.get("applied")?.as_str()?;
        [Self::Now, Self::OnResume]
            .into_iter()
            .find(|applied| applied.name() == name)
    }
}

#[cfg(test)]
mod tests;
