//! JSON-RPC 2.0 over stdio lines, and the two MCP eras (plan 8a Rulings 1
//! and 2). The protocol code lives here so a new MCP revision is one
//! file's work. Every function is total: any input gives a message or an
//! error reply, never a panic.

use serde_json::{Map, Value, json};
use std::io::{self, BufRead};

/// Invalid JSON, or a line longer than [`crate::MAX_LINE`].
pub const PARSE_ERROR: i64 = -32700;
/// JSON, but not a request (a batch, a null id, no `jsonrpc: "2.0"`).
pub const INVALID_REQUEST: i64 = -32600;
/// No such method.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// Bad params: no tool name, an unknown tool or resource.
pub const INVALID_PARAMS: i64 = -32602;
/// A caught panic.
pub const INTERNAL_ERROR: i64 = -32603;
/// A resource VizDAW could not give (it is not running, or it refused).
pub const APP_ERROR: i64 = -32000;
/// A request that came while the composer is being asked, when no more
/// can be kept until the answer.
pub const WAITING: i64 = -32001;
/// A request whose `_meta` names a protocol version not served.
pub const UNSUPPORTED_VERSION: i64 = -32022;

/// The stateless revision: no `initialize`, a version in every request.
pub const MODERN: &str = "2026-07-28";
/// The `initialize` revisions served, newest first.
pub const LEGACY: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
/// What `initialize` answers a client whose version is not served.
pub const NEWEST_LEGACY: &str = "2025-11-25";
/// Every version served, for `server/discover` and `-32022`'s data.
pub const SUPPORTED: [&str; 4] = [MODERN, "2025-11-25", "2025-06-18", "2025-03-26"];
/// Where a 2026-07-28 request names its version.
pub const VERSION_META: &str = "io.modelcontextprotocol/protocolVersion";
/// Where a 2026-07-28 request names what its client can do.
pub const CAPABILITIES_META: &str = "io.modelcontextprotocol/clientCapabilities";

/// One line, read.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    /// A request: answer it.
    Request {
        /// Its id, string or number, kept as given.
        id: Value,
        /// The method.
        method: String,
        /// Its params (empty when absent).
        params: Map<String, Value>,
    },
    /// A notification: never answered.
    Notification {
        /// The method.
        method: String,
        /// Its params (empty when absent or not an object).
        params: Map<String, Value>,
    },
    /// A response to a request this server sent (an elicitation).
    Response {
        /// The id it answers.
        id: Value,
        /// Its `result`; None for an error reply.
        result: Option<Value>,
    },
}

/// An error reply's code, message and optional data.
#[derive(Debug, Clone, PartialEq)]
pub struct RpcError {
    /// The JSON-RPC code.
    pub code: i64,
    /// One sentence.
    pub message: String,
    /// Extra data (`-32022`'s versions).
    pub data: Option<Value>,
}

impl RpcError {
    /// An error without data.
    pub fn new(code: i64, message: impl Into<String>) -> RpcError {
        RpcError {
            code,
            message: message.into(),
            data: None,
        }
    }
}

/// A result reply line.
pub fn result_line(id: &Value, result: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

/// An error reply line.
pub fn error_line(id: &Value, e: &RpcError) -> String {
    let mut error = Map::new();
    error.insert("code".into(), json!(e.code));
    error.insert("message".into(), json!(e.message));
    if let Some(d) = &e.data {
        error.insert("data".into(), d.clone());
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": error }).to_string()
}

/// A request line this server sends (an elicitation).
pub fn request_line(id: &str, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string()
}

/// A notification line this server sends.
pub fn notification_line(method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "method": method, "params": params }).to_string()
}

/// `s` cut to at most `n` characters, for messages that quote input.
pub fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Reads one line. Err is the error reply line to write (with a null id,
/// or the request's id when it had a valid one).
pub fn parse(line: &str) -> Result<Message, String> {
    let fail = |id: &Value, code, m: &str| Err(error_line(id, &RpcError::new(code, m)));
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return fail(&Value::Null, PARSE_ERROR, &format!("Parse error: {e}")),
    };
    let mut obj = match v {
        Value::Object(o) => o,
        Value::Array(_) => {
            return fail(
                &Value::Null,
                INVALID_REQUEST,
                "Batches are not supported: send one message per line.",
            );
        }
        _ => return fail(&Value::Null, INVALID_REQUEST, "A message is a JSON object."),
    };
    let id = obj.remove("id");
    let valid_id = match &id {
        Some(v @ (Value::String(_) | Value::Number(_))) => Some(v.clone()),
        _ => None,
    };
    let echo = valid_id.clone().unwrap_or(Value::Null);
    if obj.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return fail(&echo, INVALID_REQUEST, "`jsonrpc` must be \"2.0\".");
    }
    let method = match obj.remove("method") {
        Some(Value::String(m)) => m,
        Some(_) => return fail(&echo, INVALID_REQUEST, "`method` must be a string."),
        None => {
            return match valid_id {
                Some(id) if obj.contains_key("error") => Ok(Message::Response { id, result: None }),
                Some(id) if obj.contains_key("result") => Ok(Message::Response {
                    id,
                    result: obj.remove("result"),
                }),
                _ => fail(&echo, INVALID_REQUEST, "A message needs a `method`."),
            };
        }
    };
    if id.is_none() {
        let params = match obj.remove("params") {
            Some(Value::Object(p)) => p,
            _ => Map::new(),
        };
        return Ok(Message::Notification { method, params });
    }
    let Some(id) = valid_id else {
        return fail(
            &Value::Null,
            INVALID_REQUEST,
            "`id` must be a string or a number.",
        );
    };
    let params = match obj.remove("params") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(p)) => p,
        Some(_) => return fail(&id, INVALID_PARAMS, "`params` must be an object."),
    };
    Ok(Message::Request { id, method, params })
}

/// Which revision a request speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Era {
    /// After `initialize` (or a request naming an older version).
    Legacy,
    /// 2026-07-28: results carry `resultType`, and lists and reads their
    /// cache fields.
    Modern,
}

/// The era of a request with `params`; `-32022` when its `_meta` names a
/// version not served.
pub fn era(params: &Map<String, Value>) -> Result<Era, RpcError> {
    let Some(requested) = params
        .get("_meta")
        .and_then(Value::as_object)
        .and_then(|m| m.get(VERSION_META))
    else {
        return Ok(Era::Legacy);
    };
    match requested.as_str() {
        Some(MODERN) => Ok(Era::Modern),
        Some(v) if LEGACY.contains(&v) => Ok(Era::Legacy),
        _ => {
            let shown = match requested {
                Value::String(s) => Value::String(truncate(s, 64)),
                other => Value::String(truncate(&other.to_string(), 64)),
            };
            Err(RpcError {
                code: UNSUPPORTED_VERSION,
                message: "Unsupported protocol version.".into(),
                data: Some(json!({ "supported": SUPPORTED, "requested": shown })),
            })
        }
    }
}

/// What kind of result, for 2026-07-28's cache fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A list or a read: `ttlMs` and `cacheScope` too.
    Cacheable,
    /// Anything else.
    Plain,
}

/// `result` as `era` gives it: 2026-07-28 adds `resultType: "complete"`,
/// and `ttlMs: 0` and `cacheScope: "private"` to lists and reads.
pub fn finish(era: Era, kind: Kind, mut result: Value) -> Value {
    if era == Era::Modern {
        if let Some(o) = result.as_object_mut() {
            o.entry("resultType").or_insert_with(|| json!("complete"));
            if kind == Kind::Cacheable {
                o.insert("ttlMs".into(), json!(0));
                o.insert("cacheScope".into(), json!("private"));
            }
        }
    }
    result
}

/// The `initialize` answer's version: the client's when served, else the
/// newest `initialize` revision.
pub fn negotiate(requested: Option<&Value>) -> &'static str {
    let r = requested.and_then(Value::as_str);
    LEGACY
        .iter()
        .find(|v| Some(**v) == r)
        .copied()
        .unwrap_or(NEWEST_LEGACY)
}

/// Whether client capabilities `caps` declare form elicitation: an
/// `elicitation` object that is empty (form, before modes) or names `form`.
pub fn form_elicitation(caps: Option<&Value>) -> bool {
    match caps.and_then(|c| c.get("elicitation")) {
        Some(Value::Object(e)) => e.is_empty() || e.get("form").is_some_and(Value::is_object),
        _ => false,
    }
}

/// Whether a 2026-07-28 request's client declared form elicitation, in
/// `_meta` or beside it.
pub fn request_elicitation(params: &Map<String, Value>) -> bool {
    let in_meta = params.get("_meta").and_then(|m| m.get(CAPABILITIES_META));
    form_elicitation(in_meta) || form_elicitation(params.get("clientCapabilities"))
}

/// What the composer answered an elicitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// `accept` with exactly `{allow: true}`.
    Yes,
    /// Decline, cancel, an accept without `allow: true`, or an error reply.
    No,
    /// Not an elicitation result: no action, an unknown one, or content
    /// that is not the asked-for form.
    Unreadable,
}

/// Reads an elicitation's answer: `result` is the response's result (or a
/// 2026-07-28 retry's `inputResponses.yes`); None for an error reply.
/// Only an `accept` whose content is exactly `{allow: true}` is a yes.
pub fn elicitation_answer(result: Option<&Value>) -> Answer {
    let Some(r) = result else {
        return Answer::No;
    };
    let Some(o) = r.as_object() else {
        return Answer::Unreadable;
    };
    match o.get("action").and_then(Value::as_str) {
        Some("decline" | "cancel") => Answer::No,
        Some("accept") => match o.get("content") {
            None | Some(Value::Null) => Answer::No,
            Some(Value::Object(c)) if c.keys().any(|k| k != "allow") => Answer::Unreadable,
            Some(Value::Object(c)) if c.get("allow") == Some(&Value::Bool(true)) => Answer::Yes,
            Some(Value::Object(_)) => Answer::No,
            Some(_) => Answer::Unreadable,
        },
        _ => Answer::Unreadable,
    }
}

/// A line read with a length cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// The line, without its newline.
    Full(Vec<u8>),
    /// A line over the cap, consumed without being kept.
    TooLong,
}

/// Reads up to a newline; a line over `max` bytes is consumed and given as
/// [`Line::TooLong`]. None at end of input. Memory stays within `max` plus
/// one buffer, however long the line.
pub fn read_line_capped(r: &mut impl BufRead, max: usize) -> io::Result<Option<Line>> {
    let mut buf = Vec::new();
    let mut too_long = false;
    loop {
        let (used, done, eof) = {
            let available = match r.fill_buf() {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                (0, true, true)
            } else {
                match available.iter().position(|&b| b == b'\n') {
                    Some(i) => {
                        if !too_long {
                            buf.extend_from_slice(available.get(..i).unwrap_or(&[]));
                        }
                        (i + 1, true, false)
                    }
                    None => {
                        if !too_long {
                            buf.extend_from_slice(available);
                        }
                        (available.len(), false, false)
                    }
                }
            }
        };
        r.consume(used);
        if buf.len() > max {
            too_long = true;
            buf = Vec::new();
        }
        if done {
            if eof && buf.is_empty() && !too_long {
                return Ok(None);
            }
            return Ok(Some(if too_long {
                Line::TooLong
            } else {
                Line::Full(buf)
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_lines() {
        let mut r = io::BufReader::with_capacity(4, &b"ab\nabcdefghij\n\nxy"[..]);
        assert_eq!(
            read_line_capped(&mut r, 5).unwrap(),
            Some(Line::Full(b"ab".to_vec()))
        );
        assert_eq!(read_line_capped(&mut r, 5).unwrap(), Some(Line::TooLong));
        assert_eq!(
            read_line_capped(&mut r, 5).unwrap(),
            Some(Line::Full(vec![]))
        );
        assert_eq!(
            read_line_capped(&mut r, 5).unwrap(),
            Some(Line::Full(b"xy".to_vec()))
        );
        assert_eq!(read_line_capped(&mut r, 5).unwrap(), None);
    }

    #[test]
    fn answers() {
        use Answer::*;
        for (r, want) in [
            (json!({"action": "accept", "content": {"allow": true}}), Yes),
            (json!({"action": "accept", "content": {"allow": false}}), No),
            (json!({"action": "accept", "content": {"allow": 1}}), No),
            (json!({"action": "accept"}), No),
            (json!({"action": "decline"}), No),
            (json!({"action": "cancel"}), No),
            (
                json!({"action": "accept", "content": {"allow": true, "x": 1}}),
                Unreadable,
            ),
            (json!({"action": "accept", "content": [true]}), Unreadable),
            (
                json!({"action": "Accept", "content": {"allow": true}}),
                Unreadable,
            ),
            (json!({"content": {"allow": true}}), Unreadable),
            (json!(true), Unreadable),
        ] {
            assert_eq!(elicitation_answer(Some(&r)), want, "{r}");
        }
        assert_eq!(elicitation_answer(None), No);
        assert!(form_elicitation(Some(&json!({"elicitation": {}}))));
        assert!(form_elicitation(Some(
            &json!({"elicitation": {"form": {}, "url": {}}})
        )));
        assert!(!form_elicitation(Some(
            &json!({"elicitation": {"url": {}}})
        )));
        assert!(!form_elicitation(Some(&json!({"elicitation": true}))));
        assert!(!form_elicitation(None));
    }

    #[test]
    fn messages() {
        assert!(matches!(
            parse(r#"{"jsonrpc":"2.0","id":1,"result":{}}"#),
            Ok(Message::Response { .. })
        ));
        assert!(matches!(
            parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            Ok(Message::Notification { .. })
        ));
        let e = parse(r#"{"jsonrpc":"1.0","id":3,"method":"ping"}"#).unwrap_err();
        assert!(e.contains("-32600") && e.contains("\"id\":3"), "{e}");
        let e = parse(r#"{"jsonrpc":"2.0","id":[1],"method":"ping"}"#).unwrap_err();
        assert!(e.contains("-32600") && e.contains("\"id\":null"), "{e}");
        let e = parse(r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[1]}"#).unwrap_err();
        assert!(e.contains("-32602"), "{e}");
        assert_eq!(negotiate(Some(&json!("2025-03-26"))), "2025-03-26");
        assert_eq!(negotiate(Some(&json!(7))), "2025-11-25");
        assert_eq!(negotiate(None), "2025-11-25");
    }
}
