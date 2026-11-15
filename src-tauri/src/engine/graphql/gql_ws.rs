//! The `graphql-transport-ws` protocol as pure functions (plan 06b §4): building the
//! client messages and parsing the server's. No I/O, no state.

use serde_json::Value;

use crate::error::AppError;

pub const SUBPROTOCOL: &str = "graphql-transport-ws";
const INIT_PAYLOAD_MESSAGE: &str = "The subscription init payload must be a JSON object.";

#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    ConnectionAck,
    Ping(Option<Value>),
    Pong,
    Next { id: String, payload: Value },
    Error { id: String, payload: Value },
    Complete { id: String },
}

fn quoted(text: &str) -> String {
    Value::String(text.to_string()).to_string()
}

/// `Ok(None)` for an empty text, `Ok(Some(trimmed))` for a JSON object, else an error.
/// The text is kept as typed so key order and big numbers survive.
pub fn check_init_payload(text: &str) -> Result<Option<&str>, AppError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(_)) => Ok(Some(text)),
        Ok(_) => Err(AppError::invalid_input(INIT_PAYLOAD_MESSAGE)),
        Err(source) => {
            Err(AppError::invalid_input(INIT_PAYLOAD_MESSAGE).with_detail(source.to_string()))
        }
    }
}

/// `payload` must already be checked by [`check_init_payload`].
pub fn connection_init(payload: Option<&str>) -> String {
    match payload {
        Some(payload) => format!("{{\"type\":\"connection_init\",\"payload\":{payload}}}"),
        None => "{\"type\":\"connection_init\"}".to_string(),
    }
}

/// `body_json` is the `{"query", "operationName"?, "variables"?}` text.
pub fn subscribe(id: &str, body_json: &str) -> String {
    format!(
        "{{\"id\":{},\"type\":\"subscribe\",\"payload\":{body_json}}}",
        quoted(id)
    )
}

/// A pong echoes the payload of the ping it answers.
pub fn pong(payload: Option<&Value>) -> String {
    match payload {
        Some(payload) => format!("{{\"type\":\"pong\",\"payload\":{payload}}}"),
        None => "{\"type\":\"pong\"}".to_string(),
    }
}

pub fn complete(id: &str) -> String {
    format!("{{\"id\":{},\"type\":\"complete\"}}", quoted(id))
}

fn bad(message: &str) -> AppError {
    AppError::invalid_input(message)
}

/// Parses one server text frame for the operation `expected_id`.
pub fn parse(text: &str, expected_id: &str) -> Result<ServerMessage, AppError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| bad("The server sent a message that is not JSON."))?;
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("The server sent a message without a type."))?;
    let payload = value.get("payload").filter(|payload| !payload.is_null());
    let id = || -> Result<String, AppError> {
        let id = value.get("id").and_then(Value::as_str);
        match id {
            Some(id) if id == expected_id => Ok(id.to_string()),
            _ => Err(bad("The server sent a message for another operation.")),
        }
    };
    match kind {
        "connection_ack" => Ok(ServerMessage::ConnectionAck),
        "ping" => Ok(ServerMessage::Ping(payload.cloned())),
        "pong" => Ok(ServerMessage::Pong),
        "next" => Ok(ServerMessage::Next {
            id: id()?,
            payload: payload.cloned().unwrap_or(Value::Null),
        }),
        "error" => Ok(ServerMessage::Error {
            id: id()?,
            payload: payload.cloned().unwrap_or(Value::Null),
        }),
        "complete" => Ok(ServerMessage::Complete { id: id()? }),
        _ => Err(bad("The server sent a message of an unknown type.")),
    }
}

/// What a `graphql-transport-ws` close code means to the user.
pub fn close_message(code: u16) -> Option<&'static str> {
    match code {
        4400 => Some("The server rejected the request as malformed (4400)."),
        4401 => Some("The server refused the connection: unauthorized (4401)."),
        4403 => Some("The server refused the connection: forbidden (4403)."),
        4406 => Some("The server does not accept the graphql-transport-ws subprotocol (4406)."),
        4408 => Some("The server closed the connection: no connection_init in time (4408)."),
        4409 => Some("A subscriber for this id already exists (4409)."),
        4429 => Some("The server received too many connection_init requests (4429)."),
        4500 => Some("The server failed (4500)."),
        4504 => Some("The server did not acknowledge the connection in 10 s."),
        _ => None,
    }
}

#[cfg(test)]
#[path = "gql_ws_tests.rs"]
mod tests;
